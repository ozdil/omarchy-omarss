use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_ARTICLES_TOTAL: usize = 300;
const MAX_FEED_BODY_BYTES: usize = 524288; // 512 KiB
const FEED_TIMEOUT_SECS: u64 = 4;
const WHOLE_REFRESH_TIMEOUT_SECS: u64 = 15;
const MAX_CONCURRENT_FETCHES: usize = 8;

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x0001;
const POLLHUP: i16 = 0x0010;
const POLLERR: i16 = 0x0008;

extern "C" {
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn getuid() -> u32;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Feed {
    pub url: String,
    pub name: String,
    pub category: String,
    pub enabled: bool,
    pub last_fetched: String,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Article {
    pub id: String,
    pub feed_url: String,
    pub feed_name: String,
    #[serde(default)]
    pub category: String,
    pub title: String,
    pub link: String,
    pub date: String,
    pub excerpt: String,
    pub is_read: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RssState {
    pub total_articles: usize,
    pub unread_articles: usize,
    pub total_feeds: usize,
    pub feeds: Vec<Feed>,
    pub articles: Vec<Article>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BarStatus {
    pub text: String,
    pub tooltip: String,
    pub class: String,
    pub unread: usize,
}

fn hash_id(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h = (h ^ (b as u64)).wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

fn sanitize_text(s: &str, max_len: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == ' ' || *c == '\t').take(max_len).collect()
}

fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '&' {
            let mut entity = String::new();
            let mut found_semicolon = false;
            while let Some(&next_c) = chars.peek() {
                if next_c == ';' {
                    chars.next();
                    found_semicolon = true;
                    break;
                } else if next_c.is_alphanumeric() || next_c == '#' {
                    entity.push(next_c);
                    chars.next();
                    if entity.len() > 10 {
                        break;
                    }
                } else {
                    break;
                }
            }

            if found_semicolon {
                if let Some(stripped) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    if let Ok(code) = u32::from_str_radix(stripped, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                            continue;
                        }
                    }
                } else if let Some(stripped) = entity.strip_prefix('#') {
                    if let Ok(code) = stripped.parse::<u32>() {
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                            continue;
                        }
                    }
                } else {
                    match entity.as_str() {
                        "amp" => { out.push('&'); continue; }
                        "quot" => { out.push('"'); continue; }
                        "apos" => { out.push('\''); continue; }
                        "lt" => { out.push('<'); continue; }
                        "gt" => { out.push('>'); continue; }
                        "nbsp" => { out.push(' '); continue; }
                        "copy" => { out.push('©'); continue; }
                        "reg" => { out.push('®'); continue; }
                        "trade" => { out.push('™'); continue; }
                        "rsquo" | "lsquo" => { out.push('’'); continue; }
                        "rdquo" | "ldquo" => { out.push('"'); continue; }
                        "ndash" => { out.push('–'); continue; }
                        "mdash" => { out.push('—'); continue; }
                        "hellip" => { out.push('…'); continue; }
                        "ccedil" => { out.push('ç'); continue; }
                        "Ccedil" => { out.push('Ç'); continue; }
                        "ouml" => { out.push('ö'); continue; }
                        "Ouml" => { out.push('Ö'); continue; }
                        "uuml" => { out.push('ü'); continue; }
                        "Uuml" => { out.push('Ü'); continue; }
                        "bull" => { out.push('•'); continue; }
                        "deg" => { out.push('°'); continue; }
                        "euro" => { out.push('€'); continue; }
                        "pound" => { out.push('£'); continue; }
                        _ => {}
                    }
                }
                out.push('&');
                out.push_str(&entity);
                out.push(';');
            } else {
                out.push('&');
                out.push_str(&entity);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn strip_html_and_decode(s: &str, max_len: usize) -> String {
    let mut stripped = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            stripped.push(c);
        }
    }

    let decoded = decode_html_entities(&stripped);
    let mut result = String::with_capacity(decoded.len().min(max_len));
    let mut last_was_space = true;

    for c in decoded.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else if !c.is_control() {
            result.push(c);
            last_was_space = false;
        }
        if result.len() >= max_len {
            break;
        }
    }

    if result.ends_with(' ') {
        result.pop();
    }
    result
}

fn get_state_dir() -> PathBuf {
    if let Ok(state) = env::var("XDG_STATE_HOME") {
        PathBuf::from(state).join("omarchy/omarss")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home).join(".local/state/omarchy/omarss")
    } else {
        PathBuf::from("/tmp/omarss")
    }
}

fn default_feeds() -> Vec<Feed> {
    vec![
        // --- Linux & Açık Kaynak Sistemler ---
        Feed {
            url: "https://www.phoronix.com/rss.php".to_string(),
            name: "Phoronix".to_string(),
            category: "Linux & Donanım".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://lwn.net/headlines/rss".to_string(),
            name: "LWN.net".to_string(),
            category: "Linux Kernel".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "🐧".to_string(),
        },
        Feed {
            url: "https://news.itsfoss.com/rss/".to_string(),
            name: "It's FOSS".to_string(),
            category: "Linux & FOSS".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://www.omglinux.com/feed/".to_string(),
            name: "OMG! Linux".to_string(),
            category: "Linux Desktop".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://www.linuxtoday.com/feed/".to_string(),
            name: "Linux Today".to_string(),
            category: "Linux Haber".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "📰".to_string(),
        },
        Feed {
            url: "https://distrowatch.com/news/headline.xml".to_string(),
            name: "DistroWatch".to_string(),
            category: "Linux Distro".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "󰣇".to_string(),
        },
        Feed {
            url: "https://www.linux-magazine.com/rss/feed/lmi_news".to_string(),
            name: "Linux Magazine".to_string(),
            category: "Linux Dergi".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "📑".to_string(),
        },
        Feed {
            url: "https://archlinux.org/feeds/news/".to_string(),
            name: "Arch Linux".to_string(),
            category: "Linux Distro".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "󰣇".to_string(),
        },

        // --- Oyun, Steam, Steam Deck & Epic Games ---
        Feed {
            url: "https://www.gamingonlinux.com/article_rss.php".to_string(),
            name: "GamingOnLinux".to_string(),
            category: "Linux Gaming".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://store.steampowered.com/feeds/news.xml".to_string(),
            name: "Steam Official".to_string(),
            category: "Steam & Valve".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://steamdeckhq.com/feed/".to_string(),
            name: "Steam Deck HQ".to_string(),
            category: "Steam Deck".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "󰊴".to_string(),
        },
        Feed {
            url: "https://boilingsteam.com/feed/".to_string(),
            name: "Boiling Steam".to_string(),
            category: "Linux Gaming".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "♨".to_string(),
        },
        Feed {
            url: "https://www.reddit.com/r/EpicGamesPC/.rss".to_string(),
            name: "Epic Games PC".to_string(),
            category: "Epic Games".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "⚡".to_string(),
        },

        // --- Türkiye Önde Gelen Teknoloji ve Bilim Sayfaları ---
        Feed {
            url: "https://webrazzi.com/feed".to_string(),
            name: "Webrazzi".to_string(),
            category: "TR Teknoloji".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://shiftdelete.net/feed".to_string(),
            name: "ShiftDelete".to_string(),
            category: "TR Teknoloji".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://www.donanimhaber.com/rss/tum/".to_string(),
            name: "DonanımHaber".to_string(),
            category: "TR Donanım".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://www.log.com.tr/feed/".to_string(),
            name: "LOG".to_string(),
            category: "TR Teknoloji".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://www.webtekno.com/rss.xml".to_string(),
            name: "Webtekno".to_string(),
            category: "TR Teknoloji".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://evrimagaci.org/rss.xml".to_string(),
            name: "Evrim Ağacı".to_string(),
            category: "TR Bilim".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "󰄛".to_string(),
        },

        // --- Dünyada Önde Gelen Teknoloji ve Yazılım Sayfaları ---
        Feed {
            url: "https://news.ycombinator.com/rss".to_string(),
            name: "Hacker News".to_string(),
            category: "Global Tech".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
        Feed {
            url: "https://feeds.arstechnica.com/arstechnica/index".to_string(),
            name: "Ars Technica".to_string(),
            category: "Global Tech".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "📰".to_string(),
        },
        Feed {
            url: "https://www.theverge.com/rss/index.xml".to_string(),
            name: "The Verge".to_string(),
            category: "Global Tech".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "⚡".to_string(),
        },
        Feed {
            url: "https://techcrunch.com/feed/".to_string(),
            name: "TechCrunch".to_string(),
            category: "Global Tech".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "🚀".to_string(),
        },
        Feed {
            url: "https://blog.rust-lang.org/feed.xml".to_string(),
            name: "Rust Blog".to_string(),
            category: "Yazılım Dev".to_string(),
            enabled: true,
            last_fetched: "Pending".to_string(),
            icon: "".to_string(),
        },
    ]
}

const O_NOFOLLOW: i32 = 0o400000;
const MAX_FEEDS_COUNT: usize = 50;
const MAX_ARTICLES_COUNT: usize = 200;
const MAX_STATE_FILE_BYTES: u64 = 524288; // 512 KiB

static STAGING_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct TempFileGuard<'a> {
    path: &'a std::path::Path,
    active: bool,
}

impl<'a> Drop for TempFileGuard<'a> {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_file(self.path);
        }
    }
}

fn read_secure_state_file(path: &std::path::Path, max_bytes: u64) -> Option<String> {
    use std::os::unix::fs::MetadataExt;

    let parent = path.parent()?;
    if let Ok(parent_meta) = fs::symlink_metadata(parent) {
        // SAFETY: getuid is a POSIX libc function without side effects
        let current_uid = unsafe { getuid() };
        if parent_meta.file_type().is_symlink()
            || !parent_meta.file_type().is_dir()
            || parent_meta.uid() != current_uid
        {
            return None;
        }
    } else {
        return None;
    }

    // SAFETY: getuid is a POSIX libc function without side effects
    let current_uid = unsafe { getuid() };
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.file_type().is_file() || meta.uid() != current_uid {
            return None;
        }
    } else {
        return None;
    }

    let mut opts = fs::OpenOptions::new();
    opts.read(true).custom_flags(O_NOFOLLOW);

    let f = opts.open(path).ok()?;
    let meta = f.metadata().ok()?;
    if !meta.file_type().is_file() || meta.uid() != current_uid {
        return None;
    }

    let mut content = String::new();
    f.take(max_bytes).read_to_string(&mut content).ok()?;
    Some(content)
}

fn write_secure_state_file(path: &std::path::Path, content: &str) {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::sync::atomic::Ordering;

    let parent = match path.parent() {
        Some(p) => p,
        None => return,
    };

    if !parent.exists() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));

    let parent_meta = match fs::symlink_metadata(parent) {
        Ok(m) => m,
        Err(_) => return,
    };

    // SAFETY: getuid is a POSIX libc function without side effects
    let current_uid = unsafe { getuid() };
    if parent_meta.file_type().is_symlink()
        || !parent_meta.file_type().is_dir()
        || parent_meta.uid() != current_uid
    {
        return;
    }

    // If destination already exists, verify it is a regular file owned by current user
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.file_type().is_file() || meta.uid() != current_uid {
            return;
        }
    }

    let pid = std::process::id();
    let mut created_file = None;
    let mut tmp_path_buf = PathBuf::new();

    for _ in 0..10 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".tmp_state_{}_{}_{}.json", pid, nanos, seq));

        let mut opts = fs::OpenOptions::new();
        opts.write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW);

        match opts.open(&candidate) {
            Ok(f) => {
                tmp_path_buf = candidate;
                created_file = Some(f);
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return,
        }
    }

    let mut tmp_file = match created_file {
        Some(f) => f,
        None => return,
    };

    let mut guard = TempFileGuard {
        path: &tmp_path_buf,
        active: true,
    };

    let _ = tmp_file.set_permissions(fs::Permissions::from_mode(0o600));

    if tmp_file.write_all(content.as_bytes()).is_err() {
        return;
    }

    if tmp_file.sync_all().is_err() {
        return;
    }

    if let Ok(meta) = tmp_file.metadata() {
        if meta.len() != content.len() as u64 {
            return;
        }
    } else {
        return;
    }

    // Re-verify destination before rename
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.file_type().is_file() || meta.uid() != current_uid {
            return;
        }
    }

    drop(tmp_file);
    if fs::rename(&tmp_path_buf, path).is_ok() {
        guard.active = false;
    }
}

fn sanitize_and_cap_feed(feed: &mut Feed) {
    if feed.url.len() > 1024 {
        feed.url.truncate(1024);
    }
    if feed.name.len() > 128 {
        feed.name.truncate(128);
    }
    if feed.category.len() > 64 {
        feed.category.truncate(64);
    }
    if feed.icon.len() > 16 {
        feed.icon.truncate(16);
    }
    if feed.last_fetched.len() > 64 {
        feed.last_fetched.truncate(64);
    }
}

fn sanitize_and_cap_article(article: &mut Article) {
    if article.id.len() > 256 {
        article.id.truncate(256);
    }
    if article.title.len() > 256 {
        article.title.truncate(256);
    }
    if article.link.len() > 1024 {
        article.link.truncate(1024);
    }
    if article.feed_url.len() > 1024 {
        article.feed_url.truncate(1024);
    }
    if article.excerpt.len() > 2048 {
        article.excerpt.truncate(2048);
    }
    if article.feed_name.len() > 128 {
        article.feed_name.truncate(128);
    }
    if article.category.len() > 64 {
        article.category.truncate(64);
    }
    if article.date.len() > 64 {
        article.date.truncate(64);
    }
}

fn load_feeds() -> Vec<Feed> {
    let feeds_path = get_state_dir().join("feeds.json");
    if let Some(content) = read_secure_state_file(&feeds_path, MAX_STATE_FILE_BYTES) {
        if let Ok(mut feeds) = serde_json::from_str::<Vec<Feed>>(&content) {
            for f in &mut feeds {
                sanitize_and_cap_feed(f);
            }
            feeds.truncate(MAX_FEEDS_COUNT);
            if !feeds.is_empty() {
                let defs = default_feeds();
                let mut changed = false;
                for def in defs {
                    if !feeds.iter().any(|f| f.url == def.url) && feeds.len() < MAX_FEEDS_COUNT {
                        feeds.push(def);
                        changed = true;
                    }
                }
                if changed {
                    save_feeds(&feeds);
                }
                return feeds;
            }
        }
    }
    let feeds = default_feeds();
    save_feeds(&feeds);
    feeds
}

fn save_feeds(feeds: &[Feed]) {
    let mut capped = feeds.to_vec();
    for f in &mut capped {
        sanitize_and_cap_feed(f);
    }
    capped.truncate(MAX_FEEDS_COUNT);
    let feeds_path = get_state_dir().join("feeds.json");
    if let Ok(json) = serde_json::to_string_pretty(&capped) {
        write_secure_state_file(&feeds_path, &json);
    }
}

fn load_articles() -> Vec<Article> {
    let art_path = get_state_dir().join("articles.json");
    if let Some(content) = read_secure_state_file(&art_path, MAX_STATE_FILE_BYTES) {
        if let Ok(mut arts) = serde_json::from_str::<Vec<Article>>(&content) {
            for a in &mut arts {
                sanitize_and_cap_article(a);
            }
            arts.truncate(MAX_ARTICLES_COUNT);
            return arts;
        }
    }
    Vec::new()
}

fn save_articles(articles: &[Article]) {
    let mut capped = articles.to_vec();
    for a in &mut capped {
        sanitize_and_cap_article(a);
    }
    capped.truncate(MAX_ARTICLES_COUNT);
    let art_path = get_state_dir().join("articles.json");
    if let Ok(json) = serde_json::to_string_pretty(&capped) {
        write_secure_state_file(&art_path, &json);
    }
}

fn reap_process_group(mut child: std::process::Child, pid: i32) {
    // SAFETY: pid is a valid child process group ID spawned via process_group(0).
    unsafe { kill(-pid, 15); }
    std::thread::sleep(Duration::from_millis(10));
    // SAFETY: SIGKILL guarantees all processes in the isolated process group are reaped.
    unsafe { kill(-pid, 9); }
    let _ = child.wait();
}

fn fetch_url_bounded(url: &str, deadline: Instant, max_bytes: usize) -> Option<String> {
    let now = Instant::now();
    if now >= deadline {
        return None;
    }

    let remaining_secs = (deadline - now).as_secs().clamp(1, FEED_TIMEOUT_SECS);

    let mut cmd = Command::new("curl");
    cmd.args([
        "-sL",
        "--max-time",
        &remaining_secs.to_string(),
        "-H",
        "User-Agent: OmaRSS/1.0 (Omarchy Linux; +https://omarchy.org)",
        "-H",
        "Accept: application/rss+xml, application/atom+xml, text/xml, application/xml, */*",
        url,
    ])
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("LC_ALL", "C")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    cmd.process_group(0);

    let mut child = cmd.spawn().ok()?;
    let pid = child.id() as i32;
    let mut stdout = child.stdout.take()?;
    let raw_fd = stdout.as_raw_fd();

    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut timed_out = false;
    let mut overrun = false;

    loop {
        let now = Instant::now();
        if now >= deadline {
            timed_out = true;
            break;
        }
        let remaining_ms = (deadline - now).as_millis().min(50) as i32;
        let mut pfd = PollFd {
            fd: raw_fd,
            events: POLLIN | POLLHUP | POLLERR,
            revents: 0,
        };

        let ret = unsafe { poll(&mut pfd, 1, remaining_ms) };
        if ret < 0 {
            continue;
        } else if ret == 0 {
            if let Ok(Some(_)) = child.try_wait() {
                while let Ok(n) = stdout.read(&mut chunk) {
                    if n == 0 { break; }
                    if buffer.len() + n > max_bytes {
                        let take = max_bytes.saturating_sub(buffer.len());
                        buffer.extend_from_slice(&chunk[..take]);
                        overrun = true;
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..n]);
                }
                break;
            }
            continue;
        }

        if pfd.revents & POLLIN != 0 {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if buffer.len() + n > max_bytes {
                        let take = max_bytes.saturating_sub(buffer.len());
                        buffer.extend_from_slice(&chunk[..take]);
                        overrun = true;
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        } else if pfd.revents & (POLLHUP | POLLERR) != 0 {
            while let Ok(n) = stdout.read(&mut chunk) {
                if n == 0 { break; }
                if buffer.len() + n > max_bytes {
                    let take = max_bytes.saturating_sub(buffer.len());
                    buffer.extend_from_slice(&chunk[..take]);
                    overrun = true;
                    break;
                }
                buffer.extend_from_slice(&chunk[..n]);
            }
            break;
        }
    }

    if timed_out || overrun {
        reap_process_group(child, pid);
    } else {
        let _ = child.wait();
    }

    Some(String::from_utf8_lossy(&buffer).to_string())
}

fn find_open_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let mut cur = xml;
    while let Some(idx) = cur.find('<') {
        let after_bracket = &cur[idx + 1..];
        if after_bracket.starts_with(tag) {
            let next_byte = after_bracket.as_bytes().get(tag.len());
            if matches!(next_byte, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/')) {
                if let Some(tag_end) = after_bracket.find('>') {
                    return Some(&after_bracket[tag_end + 1..]);
                }
            }
        }
        cur = after_bracket;
    }
    None
}

fn find_close_tag(xml: &str, tag: &str) -> Option<usize> {
    let mut cur = xml;
    let mut offset = 0;
    while let Some(idx) = cur.find("</") {
        let after_slash = &cur[idx + 2..];
        if after_slash.starts_with(tag) {
            let next_byte = after_slash.as_bytes().get(tag.len());
            if matches!(next_byte, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')) {
                return Some(offset + idx);
            }
        }
        offset += idx + 2;
        cur = after_slash;
    }
    None
}

fn extract_xml_tag(xml: &str, tag: &str) -> String {
    if let Some(content_start) = find_open_tag(xml, tag) {
        if let Some(end_pos) = find_close_tag(content_start, tag) {
            let inner = &content_start[..end_pos];
            if let Some(cdata_start) = inner.find("<![CDATA[") {
                let after_cdata = &inner[cdata_start + 9..];
                if let Some(cdata_end) = after_cdata.find("]]>") {
                    return after_cdata[..cdata_end].to_string();
                }
            }
            return inner.to_string();
        }
    }
    String::new()
}

fn extract_atom_link(entry_xml: &str) -> String {
    let mut cur = entry_xml;
    while let Some(idx) = cur.find("<link") {
        let after = &cur[idx..];
        if let Some(end_tag) = after.find('>') {
            let tag_content = &after[..end_tag + 1];
            if tag_content.contains("href=") {
                if let Some(href_idx) = tag_content.find("href=\"") {
                    let after_href = &tag_content[href_idx + 6..];
                    if let Some(quote_end) = after_href.find('\"') {
                        let link = &after_href[..quote_end];
                        if !tag_content.contains("rel=\"self\"") {
                            return link.to_string();
                        }
                    }
                }
            }
            cur = &after[end_tag + 1..];
        } else {
            break;
        }
    }
    String::new()
}

fn parse_feed_xml(feed: &Feed, xml: &str) -> Vec<Article> {
    let mut articles = Vec::new();
    let is_atom = xml.contains("<feed") || xml.contains("<entry>");

    if is_atom {
        let mut cur = xml;
        while let Some(start) = cur.find("<entry") {
            let after_start = &cur[start..];
            if let Some(end) = after_start.find("</entry>") {
                let entry_xml = &after_start[..end + 8];
                let title = extract_xml_tag(entry_xml, "title");
                let mut link = extract_atom_link(entry_xml);
                if link.is_empty() {
                    link = extract_xml_tag(entry_xml, "link");
                }
                let mut date = extract_xml_tag(entry_xml, "updated");
                if date.is_empty() {
                    date = extract_xml_tag(entry_xml, "published");
                }
                let mut desc = extract_xml_tag(entry_xml, "summary");
                if desc.is_empty() {
                    desc = extract_xml_tag(entry_xml, "content");
                }

                if !title.is_empty() {
                    let clean_title = strip_html_and_decode(&title, 120);
                    let clean_desc = strip_html_and_decode(&desc, 250);
                    let clean_link = link.trim().to_string();
                    let clean_date = format_relative_date(&date);
                    let id = hash_id(&format!("{}{}", feed.url, if clean_link.is_empty() { &clean_title } else { &clean_link }));

                    articles.push(Article {
                        id,
                        feed_url: feed.url.clone(),
                        feed_name: feed.name.clone(),
                        category: feed.category.clone(),
                        title: clean_title,
                        link: clean_link,
                        date: clean_date,
                        excerpt: clean_desc,
                        is_read: false,
                    });
                }
                cur = &after_start[end + 8..];
            } else {
                break;
            }
            if articles.len() >= 15 {
                break;
            }
        }
    } else {
        // RSS 2.0 / RSS 1.0
        let mut cur = xml;
        while let Some(start) = cur.find("<item") {
            let after_start = &cur[start..];
            if let Some(end) = after_start.find("</item>") {
                let item_xml = &after_start[..end + 7];
                let title = extract_xml_tag(item_xml, "title");
                let link = extract_xml_tag(item_xml, "link");
                let date = extract_xml_tag(item_xml, "pubDate");
                let mut desc = extract_xml_tag(item_xml, "description");
                if desc.is_empty() {
                    desc = extract_xml_tag(item_xml, "content:encoded");
                }

                if !title.is_empty() {
                    let clean_title = strip_html_and_decode(&title, 120);
                    let clean_desc = strip_html_and_decode(&desc, 250);
                    let clean_link = link.trim().to_string();
                    let clean_date = format_relative_date(&date);
                    let id = hash_id(&format!("{}{}", feed.url, if clean_link.is_empty() { &clean_title } else { &clean_link }));

                    articles.push(Article {
                        id,
                        feed_url: feed.url.clone(),
                        feed_name: feed.name.clone(),
                        category: feed.category.clone(),
                        title: clean_title,
                        link: clean_link,
                        date: clean_date,
                        excerpt: clean_desc,
                        is_read: false,
                    });
                }
                cur = &after_start[end + 7..];
            } else {
                break;
            }
            if articles.len() >= 15 {
                break;
            }
        }
    }

    articles
}

fn format_relative_date(raw_date: &str) -> String {
    let trimmed = raw_date.trim();
    if trimmed.is_empty() {
        return "Recent".to_string();
    }
    // Handle ISO timestamps like 2026-09-08T08:30:00Z
    if trimmed.len() >= 10 && trimmed.contains('-') {
        let date_part = &trimmed[0..10];
        return date_part.to_string();
    }
    // Handle RFC 2822 timestamps like "Tue, 08 Sep 2026 07:15:00 GMT"
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 4 {
        return format!("{} {} {}", parts[1], parts[2], parts[3]);
    }
    sanitize_text(trimmed, 20)
}

fn refresh_all_feeds() -> RssState {
    let mut feeds = load_feeds();
    let existing_articles = load_articles();
    let mut read_map = std::collections::HashSet::new();
    for a in &existing_articles {
        if a.is_read {
            read_map.insert(a.id.clone());
        }
    }

    let deadline = Instant::now() + Duration::from_secs(WHOLE_REFRESH_TIMEOUT_SECS);
    let enabled_indices: Vec<usize> = feeds
        .iter()
        .enumerate()
        .filter_map(|(i, f)| if f.enabled { Some(i) } else { None })
        .collect();

    struct FetchOutcome {
        index: usize,
        last_fetched: String,
        articles: Vec<Article>,
    }

    let queue = std::sync::Mutex::new(enabled_indices.into_iter());
    let outcomes = std::sync::Mutex::new(Vec::with_capacity(feeds.len()));

    std::thread::scope(|s| {
        for _ in 0..MAX_CONCURRENT_FETCHES {
            s.spawn(|| {
                loop {
                    let next_idx = {
                        let mut lock = queue.lock().unwrap();
                        lock.next()
                    };
                    let Some(idx) = next_idx else { break; };

                    if Instant::now() >= deadline {
                        let mut lock = outcomes.lock().unwrap();
                        lock.push(FetchOutcome {
                            index: idx,
                            last_fetched: "Timeout".to_string(),
                            articles: Vec::new(),
                        });
                        continue;
                    }

                    let feed_clone = feeds[idx].clone();
                    let (status, arts) = if let Some(body) = fetch_url_bounded(&feed_clone.url, deadline, MAX_FEED_BODY_BYTES) {
                        let parsed = parse_feed_xml(&feed_clone, &body);
                        if !parsed.is_empty() {
                            ("Just now".to_string(), parsed)
                        } else {
                            ("Parsed 0".to_string(), Vec::new())
                        }
                    } else {
                        ("Timeout / Error".to_string(), Vec::new())
                    };

                    let mut lock = outcomes.lock().unwrap();
                    lock.push(FetchOutcome {
                        index: idx,
                        last_fetched: status,
                        articles: arts,
                    });
                }
            });
        }
    });

    let mut outcomes = outcomes.into_inner().unwrap();
    outcomes.sort_by_key(|o| o.index);

    let mut per_feed_articles: Vec<Vec<Article>> = Vec::with_capacity(outcomes.len());
    for o in outcomes {
        feeds[o.index].last_fetched = o.last_fetched;
        if !o.articles.is_empty() {
            let mut feed_arts = Vec::with_capacity(o.articles.len());
            for mut art in o.articles {
                if read_map.contains(&art.id) {
                    art.is_read = true;
                }
                feed_arts.push(art);
            }
            per_feed_articles.push(feed_arts);
        }
    }

    // Round-robin interleave articles across all feeds for diverse, balanced representation
    let mut new_articles = Vec::new();
    let max_feed_len = per_feed_articles.iter().map(|v| v.len()).max().unwrap_or(0);
    for i in 0..max_feed_len {
        for feed_arts in &per_feed_articles {
            if i < feed_arts.len() {
                new_articles.push(feed_arts[i].clone());
            }
        }
    }

    // Retain any existing articles that weren't in the new fetch so read state isn't lost
    let mut final_articles: Vec<Article> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for a in new_articles {
        if seen_ids.insert(a.id.clone()) {
            final_articles.push(a);
        }
    }

    for a in existing_articles {
        if seen_ids.insert(a.id.clone()) && final_articles.len() < MAX_ARTICLES_TOTAL {
            final_articles.push(a);
        }
    }

    for a in &mut final_articles {
        if a.category.is_empty() {
            if let Some(f) = feeds.iter().find(|feed| feed.url == a.feed_url) {
                a.category = f.category.clone();
            }
        }
    }

    final_articles.truncate(MAX_ARTICLES_TOTAL);

    save_feeds(&feeds);
    save_articles(&final_articles);

    let unread = final_articles.iter().filter(|a| !a.is_read).count();

    RssState {
        total_articles: final_articles.len(),
        unread_articles: unread,
        total_feeds: feeds.len(),
        feeds,
        articles: final_articles,
    }
}

fn get_current_state() -> RssState {
    let feeds = load_feeds();
    let articles = load_articles();
    let unread = articles.iter().filter(|a| !a.is_read).count();

    RssState {
        total_articles: articles.len(),
        unread_articles: unread,
        total_feeds: feeds.len(),
        feeds,
        articles,
    }
}

fn mark_article_read(target_id: &str) -> RssState {
    let feeds = load_feeds();
    let mut articles = load_articles();
    for a in &mut articles {
        if a.id == target_id {
            a.is_read = true;
        }
    }
    save_articles(&articles);
    let unread = articles.iter().filter(|a| !a.is_read).count();

    RssState {
        total_articles: articles.len(),
        unread_articles: unread,
        total_feeds: feeds.len(),
        feeds,
        articles,
    }
}

fn toggle_article_read(target_id: &str) -> RssState {
    let feeds = load_feeds();
    let mut articles = load_articles();
    for a in &mut articles {
        if a.id == target_id {
            a.is_read = !a.is_read;
        }
    }
    save_articles(&articles);
    let unread = articles.iter().filter(|a| !a.is_read).count();

    RssState {
        total_articles: articles.len(),
        unread_articles: unread,
        total_feeds: feeds.len(),
        feeds,
        articles,
    }
}

fn mark_all_articles_read() -> RssState {
    let feeds = load_feeds();
    let mut articles = load_articles();
    for a in &mut articles {
        a.is_read = true;
    }
    save_articles(&articles);

    RssState {
        total_articles: articles.len(),
        unread_articles: 0,
        total_feeds: feeds.len(),
        feeds,
        articles,
    }
}

fn add_new_feed(url: &str, custom_name: Option<&str>) -> RssState {
    let mut feeds = load_feeds();
    let clean_url = url.trim().to_string();

    if !clean_url.is_empty() && !feeds.iter().any(|f| f.url == clean_url) {
        let name = custom_name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                clean_url
                    .replace("https://", "")
                    .replace("http://", "")
                    .split('/')
                    .next()
                    .unwrap_or("Custom Feed")
                    .to_string()
            });

        feeds.push(Feed {
            url: clean_url,
            name: sanitize_text(&name, 30),
            category: "Custom".to_string(),
            enabled: true,
            last_fetched: "New".to_string(),
            icon: "".to_string(),
        });
        save_feeds(&feeds);
        return refresh_all_feeds();
    }

    get_current_state()
}

fn remove_feed(url: &str) -> RssState {
    let mut feeds = load_feeds();
    feeds.retain(|f| f.url != url);
    save_feeds(&feeds);

    let mut articles = load_articles();
    articles.retain(|a| a.feed_url != url);
    save_articles(&articles);

    let unread = articles.iter().filter(|a| !a.is_read).count();
    RssState {
        total_articles: articles.len(),
        unread_articles: unread,
        total_feeds: feeds.len(),
        feeds,
        articles,
    }
}

fn toggle_feed(url: &str) -> RssState {
    let mut feeds = load_feeds();
    for f in &mut feeds {
        if f.url == url {
            f.enabled = !f.enabled;
        }
    }
    save_feeds(&feeds);
    get_current_state()
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 && args[1] == "--open-url" {
        let url = &args[2];
        let state = if args.len() >= 4 {
            mark_article_read(&args[3])
        } else {
            get_current_state()
        };
        if url.starts_with("http://") || url.starts_with("https://") {
            let _ = Command::new("xdg-open").arg(url).spawn();
        }
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--mark-read" {
        let state = mark_article_read(&args[2]);
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--toggle-read" {
        let state = toggle_article_read(&args[2]);
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    if args.iter().any(|a| a == "--mark-all-read") {
        let state = mark_all_articles_read();
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--add-feed" {
        let name = if args.len() >= 4 { Some(args[3].as_str()) } else { None };
        let state = add_new_feed(&args[2], name);
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--remove-feed" {
        let state = remove_feed(&args[2]);
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--toggle-feed" {
        let state = toggle_feed(&args[2]);
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    if args.iter().any(|a| a == "--reset-feeds") {
        let feeds = default_feeds();
        save_feeds(&feeds);
        let state = refresh_all_feeds();
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    let state = if args.iter().any(|a| a == "--refresh") {
        refresh_all_feeds()
    } else {
        let current = get_current_state();
        if current.total_articles == 0 {
            refresh_all_feeds()
        } else {
            current
        }
    };

    if args.iter().any(|a| a == "--status") {
        let text = "".to_string();
        let tooltip = format!(
            "OmaRSS - Feed Reader\nUnread: {} articles\nActive Feeds: {} / {}\n\n[Left Click] Open Reader",
            state.unread_articles,
            state.feeds.iter().filter(|f| f.enabled).count(),
            state.total_feeds
        );
        let out = BarStatus {
            text,
            tooltip,
            class: if state.unread_articles > 0 { "highlight".to_string() } else { "normal".to_string() },
            unread: state.unread_articles,
        };
        println!("{}", serde_json::to_string(&out).unwrap());
        return;
    }

    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string(&state).unwrap());
        return;
    }

    println!("OMARSS - NATIVE FEED READER");
    println!("Total Articles: {} | Unread: {} | Subscribed Feeds: {}", state.total_articles, state.unread_articles, state.total_feeds);
    println!("{:<20} {:<12} {:<45}", "FEED", "DATE", "TITLE");
    println!("{}", "-".repeat(80));
    for a in state.articles.iter().take(20) {
        let read_badge = if a.is_read { " " } else { "●" };
        println!("{} {:<18} {:<12} {:<45}", read_badge, a.feed_name, a.date, a.title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_html_entities() {
        assert_eq!(decode_html_entities("&amp; &quot; &apos; &lt; &gt;"), "& \" ' < >");
        assert_eq!(decode_html_entities("Türkiye&#8217;nin"), "Türkiye’nin");
        assert_eq!(decode_html_entities("&ccedil;&ouml;&uuml;&Ccedil;&Ouml;&Uuml;"), "çöüÇÖÜ");
        assert_eq!(decode_html_entities("Sam Altman&#8217;a"), "Sam Altman’a");
        assert_eq!(decode_html_entities("test &ndash; test"), "test – test");
        assert_eq!(decode_html_entities("price: &euro;50"), "price: €50");
    }

    #[test]
    fn test_strip_html_and_decode() {
        let input = "<p>Hello <b>World</b> &amp; <i>Rust</i>!</p>";
        assert_eq!(strip_html_and_decode(input, 50), "Hello World & Rust!");
    }

    #[test]
    fn test_extract_xml_tag() {
        let xml = "<item><title><![CDATA[Test Title With CDATA]]></title><link>https://example.com</link></item>";
        assert_eq!(extract_xml_tag(xml, "title"), "Test Title With CDATA");
        assert_eq!(extract_xml_tag(xml, "link"), "https://example.com");
    }

    #[test]
    fn test_default_feeds_validity() {
        let feeds = default_feeds();
        assert!(feeds.len() >= 10);
        for f in &feeds {
            assert!(f.url.starts_with("https://") || f.url.starts_with("http://"));
            assert!(!f.name.is_empty());
            assert!(!f.category.is_empty());
            assert!(!f.icon.is_empty());
        }
    }

    #[test]
    fn test_parse_feed_xml_rss2() {
        let feed = Feed {
            url: "https://example.com/rss".to_string(),
            name: "Webrazzi".to_string(),
            category: "TR Teknoloji".to_string(),
            enabled: true,
            last_fetched: "Never".to_string(),
            icon: "".to_string(),
        };
        let xml = r#"<?xml version="1.0"?>
        <rss version="2.0">
            <channel>
                <item>
                    <title>Yeni Yapay Zeka Modeli Tanıtıldı</title>
                    <link>https://example.com/ai-model</link>
                    <pubDate>Tue, 08 Sep 2026 12:00:00 GMT</pubDate>
                    <description>Yeni model özellikleri duyuruldu.</description>
                </item>
            </channel>
        </rss>"#;
        let articles = parse_feed_xml(&feed, xml);
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].title, "Yeni Yapay Zeka Modeli Tanıtıldı");
        assert_eq!(articles[0].link, "https://example.com/ai-model");
        assert_eq!(articles[0].category, "TR Teknoloji");
        assert_eq!(articles[0].feed_name, "Webrazzi");
    }

    #[test]
    fn test_parse_feed_xml_atom() {
        let feed = Feed {
            url: "https://example.com/atom".to_string(),
            name: "The Verge".to_string(),
            category: "Global Tech".to_string(),
            enabled: true,
            last_fetched: "Never".to_string(),
            icon: "⚡".to_string(),
        };
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
            <entry>
                <title>Global Tech Breakthrough</title>
                <link rel="alternate" type="text/html" href="https://example.com/breakthrough"/>
                <published>2026-09-08T10:00:00Z</published>
                <summary>A groundbreaking achievement in computing.</summary>
            </entry>
        </feed>"#;
        let articles = parse_feed_xml(&feed, xml);
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].title, "Global Tech Breakthrough");
        assert_eq!(articles[0].link, "https://example.com/breakthrough");
        assert_eq!(articles[0].category, "Global Tech");
    }

    #[test]
    fn test_format_relative_date() {
        assert_eq!(format_relative_date("2026-09-08T12:30:00Z"), "2026-09-08");
        assert_eq!(format_relative_date("Tue, 08 Sep 2026 14:30:00 +0000"), "08 Sep 2026");
        assert_eq!(format_relative_date(""), "Recent");
    }

    #[test]
    fn test_extract_xml_tag_with_attributes() {
        let xml = r#"<entry><title type="text">Custom Attribute Title</title><content type="html"><![CDATA[<b>Bold text</b>]]></content></entry>"#;
        assert_eq!(extract_xml_tag(xml, "title"), "Custom Attribute Title");
        assert_eq!(extract_xml_tag(xml, "content"), "<b>Bold text</b>");
    }

    #[test]
    fn test_strip_html_whitespace_normalization() {
        let input = "<p>First paragraph.</p>   \n\n\t  <p>Second &amp; final.</p>";
        assert_eq!(strip_html_and_decode(input, 100), "First paragraph. Second & final.");
    }

    #[test]
    fn test_secure_state_file_write_and_read_roundtrip() {
        let tmp_dir = std::env::temp_dir().join(format!("omarss_test_roundtrip_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp_dir);
        let target = tmp_dir.join("test_state.json");

        let sample_data = r#"{"test":"data_roundtrip_ok"}"#;
        write_secure_state_file(&target, sample_data);

        let read_back = read_secure_state_file(&target, MAX_STATE_FILE_BYTES);
        assert_eq!(read_back, Some(sample_data.to_string()));

        // Verify mode 0600 permissions
        use std::os::unix::fs::MetadataExt;
        let meta = fs::symlink_metadata(&target).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o600);

        let _ = fs::remove_file(&target);
        let _ = fs::remove_dir(&tmp_dir);
    }

    #[test]
    fn test_secure_state_file_symlink_rejection() {
        let tmp_dir = std::env::temp_dir().join(format!("omarss_test_symlink_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp_dir);
        let victim_file = tmp_dir.join("victim.txt");
        fs::write(&victim_file, "PRESERVE_VICTIM_CONTENT").unwrap();

        let symlink_target = tmp_dir.join("symlink_target.json");
        std::os::unix::fs::symlink(&victim_file, &symlink_target).unwrap();

        // Reading through a symlink must fail
        assert_eq!(read_secure_state_file(&symlink_target, MAX_STATE_FILE_BYTES), None);

        // Writing through a symlink must fail and preserve victim file
        write_secure_state_file(&symlink_target, "MALICIOUS_OVERWRITE");
        let victim_content = fs::read_to_string(&victim_file).unwrap();
        assert_eq!(victim_content, "PRESERVE_VICTIM_CONTENT");

        let _ = fs::remove_file(&symlink_target);
        let _ = fs::remove_file(&victim_file);
        let _ = fs::remove_dir(&tmp_dir);
    }

    #[test]
    fn test_preplanted_tmp_symlink_rejection() {
        let tmp_dir = std::env::temp_dir().join(format!("omarss_test_preplanted_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp_dir);
        let victim_file = tmp_dir.join("victim.txt");
        fs::write(&victim_file, "VICTIM_INTACT").unwrap();

        // Pre-plant temporary symlink
        let preplanted_symlink = tmp_dir.join(format!(".tmp_state_{}_0_0.json", std::process::id()));
        std::os::unix::fs::symlink(&victim_file, &preplanted_symlink).unwrap();

        let target = tmp_dir.join("feeds.json");
        write_secure_state_file(&target, r#"[{"url":"https://test.com"}]"#);

        // Victim content must remain unchanged
        let victim_content = fs::read_to_string(&victim_file).unwrap();
        assert_eq!(victim_content, "VICTIM_INTACT");

        let _ = fs::remove_file(&preplanted_symlink);
        let _ = fs::remove_file(&target);
        let _ = fs::remove_file(&victim_file);
        let _ = fs::remove_dir(&tmp_dir);
    }

    #[test]
    fn test_collection_and_field_caps() {
        let mut feed = Feed {
            url: "a".repeat(2000),
            name: "b".repeat(500),
            category: "c".repeat(200),
            enabled: true,
            last_fetched: "d".repeat(200),
            icon: "e".repeat(50),
        };
        sanitize_and_cap_feed(&mut feed);
        assert_eq!(feed.url.len(), 1024);
        assert_eq!(feed.name.len(), 128);
        assert_eq!(feed.category.len(), 64);
        assert_eq!(feed.icon.len(), 16);
        assert_eq!(feed.last_fetched.len(), 64);

        let mut article = Article {
            id: "x".repeat(500),
            title: "t".repeat(500),
            link: "l".repeat(2000),
            feed_url: "u".repeat(2000),
            date: "d".repeat(200),
            feed_name: "f".repeat(300),
            category: "c".repeat(200),
            excerpt: "s".repeat(5000),
            is_read: true,
        };
        sanitize_and_cap_article(&mut article);
        assert_eq!(article.id.len(), 256);
        assert_eq!(article.title.len(), 256);
        assert_eq!(article.link.len(), 1024);
        assert_eq!(article.feed_url.len(), 1024);
        assert_eq!(article.excerpt.len(), 2048);
        assert_eq!(article.feed_name.len(), 128);
        assert_eq!(article.category.len(), 64);
    }

    #[test]
    fn test_bounded_state_file_read_cap() {
        let tmp_dir = std::env::temp_dir().join(format!("omarss_test_read_cap_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp_dir);
        let oversized_file = tmp_dir.join("oversized.json");
        let large_content = "X".repeat(1024 * 1024); // 1 MiB
        fs::write(&oversized_file, &large_content).unwrap();

        // Reading with 100 byte cap must return at most 100 bytes
        let read_result = read_secure_state_file(&oversized_file, 100);
        assert!(read_result.is_some());
        assert_eq!(read_result.unwrap().len(), 100);

        let _ = fs::remove_file(&oversized_file);
        let _ = fs::remove_dir(&tmp_dir);
    }
}
