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
const WHOLE_REFRESH_TIMEOUT_SECS: u64 = 30;

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
    let decoded_once = decode_html_entities(s);
    let mut clean = String::with_capacity(decoded_once.len());
    let mut in_tag = false;

    for c in decoded_once.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            clean.push(c);
        }
    }

    let decoded_again = decode_html_entities(&clean);
    let words: Vec<&str> = decoded_again.split_whitespace().collect();
    let res = words.join(" ");
    sanitize_text(&res, max_len)
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

fn load_feeds() -> Vec<Feed> {
    let feeds_path = get_state_dir().join("feeds.json");
    if let Ok(content) = fs::read_to_string(&feeds_path) {
        if let Ok(mut feeds) = serde_json::from_str::<Vec<Feed>>(&content) {
            if !feeds.is_empty() {
                let defs = default_feeds();
                let mut changed = false;
                for def in defs {
                    if !feeds.iter().any(|f| f.url == def.url) {
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
    let dir = get_state_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let feeds_path = dir.join("feeds.json");
    if let Ok(json) = serde_json::to_string_pretty(feeds) {
        let tmp = dir.join(".tmp_feeds.json");
        if let Ok(mut file) = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
        {
            use std::io::Write;
            if file.write_all(json.as_bytes()).is_ok() && file.sync_all().is_ok() {
                drop(file);
                let _ = fs::rename(tmp, feeds_path);
            }
        }
    }
}

fn load_articles() -> Vec<Article> {
    let art_path = get_state_dir().join("articles.json");
    if let Ok(content) = fs::read_to_string(&art_path) {
        if let Ok(arts) = serde_json::from_str::<Vec<Article>>(&content) {
            return arts;
        }
    }
    Vec::new()
}

fn save_articles(articles: &[Article]) {
    let dir = get_state_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let art_path = dir.join("articles.json");
    if let Ok(json) = serde_json::to_string_pretty(articles) {
        let tmp = dir.join(".tmp_articles.json");
        if let Ok(mut file) = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
        {
            use std::io::Write;
            if file.write_all(json.as_bytes()).is_ok() && file.sync_all().is_ok() {
                drop(file);
                let _ = fs::rename(tmp, art_path);
            }
        }
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

fn extract_xml_tag(xml: &str, tag: &str) -> String {
    let open_tag = format!("<{}", tag);
    let close_tag = format!("</{}>", tag);

    if let Some(start_pos) = xml.find(&open_tag) {
        let after_open = &xml[start_pos + open_tag.len()..];
        if let Some(tag_end) = after_open.find('>') {
            let content_start = &after_open[tag_end + 1..];
            if let Some(end_pos) = content_start.find(&close_tag) {
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
    let mut per_feed_articles: Vec<Vec<Article>> = Vec::new();

    for feed in &mut feeds {
        if !feed.enabled || Instant::now() >= deadline {
            continue;
        }

        if let Some(body) = fetch_url_bounded(&feed.url, deadline, MAX_FEED_BODY_BYTES) {
            let parsed = parse_feed_xml(feed, &body);
            if !parsed.is_empty() {
                feed.last_fetched = "Just now".to_string();
                let mut feed_arts = Vec::new();
                for mut art in parsed {
                    if read_map.contains(&art.id) {
                        art.is_read = true;
                    }
                    feed_arts.push(art);
                }
                per_feed_articles.push(feed_arts);
            } else {
                feed.last_fetched = "Parsed 0".to_string();
            }
        } else {
            feed.last_fetched = "Timeout / Error".to_string();
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
        if args.len() >= 4 {
            let _ = mark_article_read(&args[3]);
        }
        if url.starts_with("http://") || url.starts_with("https://") {
            let _ = Command::new("xdg-open").arg(url).spawn();
        }
        return;
    }

    if args.len() >= 3 && args[1] == "--mark-read" {
        let state = mark_article_read(&args[2]);
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--toggle-read" {
        let state = toggle_article_read(&args[2]);
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
        return;
    }

    if args.iter().any(|a| a == "--mark-all-read") {
        let state = mark_all_articles_read();
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--add-feed" {
        let name = if args.len() >= 4 { Some(args[3].as_str()) } else { None };
        let state = add_new_feed(&args[2], name);
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--remove-feed" {
        let state = remove_feed(&args[2]);
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
        return;
    }

    if args.len() >= 3 && args[1] == "--toggle-feed" {
        let state = toggle_feed(&args[2]);
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
        return;
    }

    if args.iter().any(|a| a == "--reset-feeds") {
        let feeds = default_feeds();
        save_feeds(&feeds);
        let state = refresh_all_feeds();
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
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
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
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
}
