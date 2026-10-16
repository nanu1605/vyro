//! Power-event history from the Windows System event log.
//!
//! Events are rendered to XML by `EvtRender`; only a handful of fields are needed, so they are
//! extracted with plain string scanning instead of an XML parser.

use crate::info::{PowerEvent, PowerEventKind};

/// XPath selecting the events [`classify`] understands.
pub const QUERY: &str = "*[System[\
(Provider[@Name='Microsoft-Windows-Kernel-General'] and (EventID=12 or EventID=13)) or \
(Provider[@Name='Microsoft-Windows-Kernel-Power'] and (EventID=41 or EventID=42 or EventID=506 or EventID=507)) or \
(Provider[@Name='Microsoft-Windows-Power-Troubleshooter'] and EventID=1)]]";

/// Value of `attr` on the first `<tag …>` element.
fn attr<'a>(xml: &'a str, tag: &str, attr: &str) -> Option<&'a str> {
    let start = xml.find(&format!("<{tag} "))?;
    let end = start + xml[start..].find('>')?;
    let el = &xml[start..end];
    let key = format!("{attr}=");
    let at = el.find(&key)? + key.len();
    let quote = el[at..].chars().next()?;
    let rest = &el[at + 1..];
    Some(&rest[..rest.find(quote)?])
}

/// Text of the first `<tag>…</tag>` (attributes allowed on the open tag).
fn text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = xml.find(&format!("<{tag}")).map(|i| i + 1 + tag.len())?;
    let body = open + xml[open..].find('>')? + 1;
    let end = body + xml[body..].find(&format!("</{tag}>"))?;
    Some(xml[body..end].trim())
}

/// Text of `<Data Name='name'>…</Data>`.
fn data<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['\'', '"'] {
        let open = format!("<Data Name={quote}{name}{quote}>");
        if let Some(i) = xml.find(&open) {
            let body = i + open.len();
            let end = body + xml[body..].find("</Data>")?;
            return Some(xml[body..end].trim());
        }
    }
    None
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fffffff]Z` (UTC) to Unix seconds.
pub fn parse_utc(s: &str) -> Option<i64> {
    let s = s.trim().strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(str::parse::<i64>);
    let (y, mo, da) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let time = time.split('.').next()?;
    let mut t = time.split(':').map(str::parse::<i64>);
    let (h, mi, se) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + se)
}

pub fn classify(provider: &str, id: u32) -> Option<PowerEventKind> {
    let p = provider
        .strip_prefix("Microsoft-Windows-")
        .unwrap_or(provider);
    Some(match (p, id) {
        ("Kernel-General", 12) => PowerEventKind::Boot,
        ("Kernel-General", 13) => PowerEventKind::Shutdown,
        ("Kernel-Power", 41) => PowerEventKind::UnexpectedShutdown,
        ("Kernel-Power", 42 | 506) => PowerEventKind::Sleep,
        ("Kernel-Power", 507) | ("Power-Troubleshooter", 1) => PowerEventKind::Wake,
        _ => return None,
    })
}

/// What the Performance Score's Stability input counts (P17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StabilityKind {
    /// Kernel-Power 41: the PC stopped without shutting down.
    Shutdown,
    /// WHEA-Logger 17, 19, 47: a hardware error the CPU or chipset corrected.
    Corrected,
    /// WHEA-Logger 18: a fatal hardware error.
    Fatal,
}

/// XPath for [`StabilityKind`] events of the last `secs` seconds.
pub fn stability_query(secs: i64) -> String {
    format!(
        "*[System[((Provider[@Name='Microsoft-Windows-Kernel-Power'] and EventID=41) or \
         (Provider[@Name='Microsoft-Windows-WHEA-Logger'] and (EventID=17 or EventID=18 or \
         EventID=19 or EventID=47))) and TimeCreated[timediff(@SystemTime) <= {}]]]",
        secs.max(0) * 1000
    )
}

pub fn stability_kind(provider: &str, id: u32) -> Option<StabilityKind> {
    let p = provider
        .strip_prefix("Microsoft-Windows-")
        .unwrap_or(provider);
    Some(match (p, id) {
        ("Kernel-Power", 41) => StabilityKind::Shutdown,
        ("WHEA-Logger", 17 | 19 | 47) => StabilityKind::Corrected,
        ("WHEA-Logger", 18) => StabilityKind::Fatal,
        _ => return None,
    })
}

/// Decodes one rendered event for the Stability input: when, and what.
pub fn parse_stability(xml: &str) -> Option<(i64, StabilityKind)> {
    let provider = attr(xml, "Provider", "Name")?;
    let id: u32 = text(xml, "EventID")?.parse().ok()?;
    let ts = parse_utc(attr(xml, "TimeCreated", "SystemTime")?)?;
    Some((ts, stability_kind(provider, id)?))
}

/// Counts the events newer than `since`.
pub fn count_stability(
    events: impl IntoIterator<Item = (i64, StabilityKind)>,
    since: i64,
) -> crate::score::StabilityCounts {
    let mut c = crate::score::StabilityCounts::default();
    for (_, kind) in events.into_iter().filter(|(ts, _)| *ts >= since) {
        let n = match kind {
            StabilityKind::Shutdown => &mut c.shutdowns,
            StabilityKind::Corrected => &mut c.corrected,
            StabilityKind::Fatal => &mut c.fatal,
        };
        *n = n.saturating_add(1);
    }
    c
}

/// Decodes one rendered event.
pub fn parse_event(xml: &str) -> Option<PowerEvent> {
    let provider = attr(xml, "Provider", "Name")?;
    let id: u32 = text(xml, "EventID")?.parse().ok()?;
    let ts = parse_utc(attr(xml, "TimeCreated", "SystemTime")?)?;
    let kind = classify(provider, id)?;
    let detail = match (kind, id) {
        (PowerEventKind::Wake, 1) => data(xml, "WakeSourceText")
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("unknown"))
            .map(|s| format!("wake source: {s}")),
        (PowerEventKind::Sleep, 506) | (PowerEventKind::Wake, 507) => {
            Some("modern standby".to_owned())
        }
        _ => None,
    };
    Some(PowerEvent { ts, kind, detail })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAKE: &str = "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System>\
        <Provider Name='Microsoft-Windows-Power-Troubleshooter' Guid='{cdc05e28-c449-49c6-b9d2-88cf761644df}'/>\
        <EventID>1</EventID><Version>3</Version><Level>4</Level>\
        <TimeCreated SystemTime='2026-09-25T07:12:03.5549012Z'/></System>\
        <EventData><Data Name='SleepTime'>2026-09-24T23:01:40.1Z</Data>\
        <Data Name='WakeSourceText'>USB Composite Device</Data></EventData></Event>";

    const UNEXPECTED: &str = "<Event><System><Provider Name=\"Microsoft-Windows-Kernel-Power\" Guid=\"{x}\"/>\
        <EventID Qualifiers=\"0\">41</EventID><TimeCreated SystemTime=\"2024-02-29T12:00:00.0Z\"/></System></Event>";

    #[test]
    fn stability_events_are_counted_by_kind() {
        let whea = |id: u32, at: &str| {
            format!(
                "<Event><System><Provider Name='Microsoft-Windows-WHEA-Logger' Guid='{{x}}'/>\
                 <EventID>{id}</EventID><TimeCreated SystemTime='{at}'/></System></Event>"
            )
        };
        let events: Vec<(i64, StabilityKind)> = [
            UNEXPECTED.to_owned(),
            whea(19, "2024-03-01T08:00:00.0Z"),
            whea(47, "2024-03-02T08:00:00.0Z"),
            whea(18, "2024-03-03T08:00:00.0Z"),
            whea(20, "2024-03-03T09:00:00.0Z"),
            WAKE.to_owned(),
        ]
        .iter()
        .filter_map(|x| parse_stability(x))
        .collect();
        assert_eq!(events.len(), 4, "a WHEA 20 and a wake are not counted");
        let c = count_stability(events.clone(), 0);
        assert_eq!((c.shutdowns, c.corrected, c.fatal), (1, 2, 1));
        // Older than the window: left out.
        let since = parse_utc("2024-03-02T00:00:00Z").unwrap();
        let c = count_stability(events, since);
        assert_eq!((c.shutdowns, c.corrected, c.fatal), (0, 1, 1));
        assert!(stability_query(30 * 86_400).contains("timediff(@SystemTime) <= 2592000000"));
    }

    #[test]
    fn iso_utc_to_unix() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2000-03-01T00:00:00.9999999Z"), Some(951_868_800));
        assert_eq!(parse_utc("2024-02-29T12:00:00.0Z"), Some(1_709_208_000));
        assert_eq!(parse_utc("2024-13-01T00:00:00Z"), None);
        assert_eq!(parse_utc("2024-01-01 00:00:00"), None);
    }

    #[test]
    fn wake_event_with_source() {
        let e = parse_event(WAKE).unwrap();
        assert_eq!(e.kind, PowerEventKind::Wake);
        assert_eq!(e.ts, parse_utc("2026-09-25T07:12:03Z").unwrap());
        assert_eq!(
            e.detail.as_deref(),
            Some("wake source: USB Composite Device")
        );
    }

    #[test]
    fn double_quoted_attributes_and_qualified_event_id() {
        let e = parse_event(UNEXPECTED).unwrap();
        assert_eq!(e.kind, PowerEventKind::UnexpectedShutdown);
        assert_eq!(e.ts, 1_709_208_000);
    }

    #[test]
    fn unrelated_events_are_ignored() {
        let xml = WAKE.replace("Power-Troubleshooter", "Kernel-Boot");
        assert!(parse_event(&xml).is_none());
        assert!(parse_event("<Event/>").is_none());
    }
}
