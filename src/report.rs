use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, BufRead, BufReader, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    OfficeForeground,
    DialogOpened,
    DialogClosed,
    ForegroundCandidate,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    OfficeProcess,
    OfficeOwner,
    Temporal,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    pub utc_ms: u64,
    pub elapsed_ms: u64,
    pub window_id: u64,
    pub kind: Kind,
    pub pid: u32,
    pub process: Option<String>,
    pub class: String,
    pub owner_pid: Option<u32>,
    pub owner_process: Option<String>,
    pub relation: Relation,
}

#[cfg(windows)]
pub fn write_event(mut output: impl Write, event: &Event) -> io::Result<()> {
    serde_json::to_writer(&mut output, event)?;
    writeln!(output)
}

pub fn print_event(mut output: impl Write, event: &Event) -> io::Result<()> {
    let kind = match event.kind {
        Kind::OfficeForeground => "Office foreground",
        Kind::DialogOpened => "dialog opened",
        Kind::DialogClosed => "dialog closed",
        Kind::ForegroundCandidate => "foreground candidate",
    };
    let relation = match event.relation {
        Relation::OfficeProcess => "Office process",
        Relation::OfficeOwner => "Office owner",
        Relation::Temporal => "time proximity",
    };
    write!(
        output,
        "+{}.{:03}s  {kind:<20} pid={} process={:?} class={:?} [{relation}]",
        event.elapsed_ms / 1000,
        event.elapsed_ms % 1000,
        event.pid,
        event.process.as_deref().unwrap_or("unknown"),
        event.class,
    )?;
    if let Some(pid) = event.owner_pid {
        write!(
            output,
            " owner_pid={pid} owner={:?}",
            event.owner_process.as_deref().unwrap_or("unknown")
        )?;
    }
    writeln!(output)
}

#[derive(Default)]
struct DialogStats {
    active: BTreeSet<u64>,
    last_close: Option<u64>,
    openings: u64,
    reopenings: u64,
    last_gap: Option<u64>,
}

#[derive(Default)]
pub struct Summary {
    groups: BTreeMap<(Option<String>, u32, String), DialogStats>,
    candidates: u64,
    office_events: u64,
}

impl Summary {
    pub fn record(&mut self, event: &Event) {
        match event.kind {
            Kind::OfficeForeground => self.office_events += 1,
            Kind::ForegroundCandidate => self.candidates += 1,
            Kind::DialogOpened | Kind::DialogClosed => {
                let key = (
                    event.process.as_ref().map(|name| name.to_ascii_lowercase()),
                    event.pid,
                    event.class.clone(),
                );
                let group = self.groups.entry(key).or_default();
                if event.kind == Kind::DialogOpened {
                    if !group.active.insert(event.window_id) {
                        return;
                    }
                    group.openings += 1;
                    if group.active.len() == 1 {
                        if let Some(gap) = group
                            .last_close
                            .take()
                            .and_then(|closed| event.elapsed_ms.checked_sub(closed))
                        {
                            group.reopenings += 1;
                            group.last_gap = Some(gap);
                        }
                    }
                } else if group.active.remove(&event.window_id) && group.active.is_empty() {
                    group.last_close = Some(event.elapsed_ms);
                }
            }
        }
    }

    pub fn print(&self, mut output: impl Write) -> io::Result<()> {
        writeln!(
            output,
            "Office foreground events: {}; foreground candidates: {}",
            self.office_events, self.candidates
        )?;
        let mut has_openings = false;
        for ((process, pid, class), stats) in &self.groups {
            if stats.openings == 0 {
                continue;
            }
            has_openings = true;
            write!(
                output,
                "pid={pid} process={:?} class={class:?} openings={} reopenings={}",
                process.as_deref().unwrap_or("unknown"),
                stats.openings,
                stats.reopenings
            )?;
            if let Some(gap) = stats.last_gap {
                write!(output, " last_reopen_gap_ms={gap}")?;
            }
            writeln!(output)?;
        }
        if !has_openings {
            writeln!(output, "No tracked dialog openings.")?;
        }
        Ok(())
    }
}

pub fn summarize_file(path: &Path, mut output: impl Write) -> io::Result<()> {
    let mut summary = Summary::default();
    for (index, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str(&line).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid JSONL at line {}: {error}", index + 1),
            )
        })?;
        print_event(&mut output, &event)?;
        summary.record(&event);
    }
    writeln!(output)?;
    summary.print(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: Kind, window_id: u64, elapsed_ms: u64) -> Event {
        Event {
            utc_ms: 1_700_000_000_000 + elapsed_ms,
            elapsed_ms,
            window_id,
            kind,
            pid: 123,
            process: Some("ExampleAuth.exe".into()),
            class: "#32770".into(),
            owner_pid: Some(345),
            owner_process: Some("WINWORD.EXE".into()),
            relation: Relation::OfficeOwner,
        }
    }

    #[test]
    fn counts_a_new_opening_after_close_and_ignores_duplicates_and_foreground() {
        let mut summary = Summary::default();
        for event in [
            event(Kind::DialogOpened, 1, 100),
            event(Kind::DialogOpened, 1, 101),
            event(Kind::ForegroundCandidate, 1, 200),
            event(Kind::DialogClosed, 1, 300),
            event(Kind::DialogClosed, 1, 301),
            event(Kind::DialogOpened, 2, 425),
        ] {
            summary.record(&event);
        }
        let group = summary.groups.values().next().unwrap();
        assert_eq!(group.openings, 2);
        assert_eq!(group.reopenings, 1);
        assert_eq!(group.last_gap, Some(125));
        assert_eq!(summary.candidates, 1);
    }

    #[test]
    fn overlapping_dialogs_do_not_count_as_reopening() {
        let mut summary = Summary::default();
        for event in [
            event(Kind::DialogOpened, 1, 100),
            event(Kind::DialogOpened, 2, 200),
            event(Kind::DialogClosed, 1, 300),
            event(Kind::DialogOpened, 3, 400),
        ] {
            summary.record(&event);
        }
        let group = summary.groups.values().next().unwrap();
        assert_eq!(group.openings, 3);
        assert_eq!(group.reopenings, 0);
    }
}
