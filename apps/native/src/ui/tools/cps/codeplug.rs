use sdrmm_wire::cps::{Codeplug, CpsCodeplugDetail};
use zgui::prelude::*;
use zgui_ui::prelude::*;

use super::logic::{
    channel_detail, channel_kind, contact_kind_label, format_mhz, format_shift, kind_label,
    power_label, zone_channels,
};
use crate::ui::{tools::kit::Query, widgets::segments};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Channels,
    Zones,
    Contacts,
    Groups,
    Scans,
    Ids,
}

const SECTIONS: [(Section, &str); 6] = [
    (Section::Channels, "Channels"),
    (Section::Zones, "Zones"),
    (Section::Contacts, "Contacts"),
    (Section::Groups, "Group lists"),
    (Section::Scans, "Scan lists"),
    (Section::Ids, "Radio IDs"),
];

pub fn view(codeplug: Query<CpsCodeplugDetail>) -> impl IntoView {
    let section = RwSignal::new(Section::Channels);
    let table = move || {
        let chosen = section.get();
        codeplug.data.with(|data| {
            data.as_ref()
                .map(|detail| AnyView::new(table(&detail.codeplug, chosen)))
        })
    };
    view! {
        column(class = "cps__work") {
            {segments(SECTIONS.to_vec(), section.into(), move |picked| section.set(picked))}
            box(class = "tool-scroll") {{table}}
        }
    }
}

fn table(codeplug: &Codeplug, section: Section) -> impl IntoView {
    match section {
        Section::Channels => grid(
            "cps__channels",
            &[
                "#",
                "Name",
                "RX MHz",
                "Shift",
                "Mode",
                "Power",
                "Detail",
                "Scan list",
            ],
            channel_rows(codeplug),
        ),
        Section::Zones => grid(
            "cps__two",
            &["Zone", "Channels"],
            members(
                codeplug
                    .zones
                    .iter()
                    .map(|zone| (zone.name.clone(), zone_channels(zone))),
            ),
        ),
        Section::Contacts => grid(
            "cps__three",
            &["Name", "Kind", "Number"],
            codeplug
                .contacts
                .iter()
                .map(|contact| {
                    vec![
                        Cell::new(contact.name.clone(), ""),
                        Cell::new(contact_kind_label(contact.kind), "tool-dim"),
                        Cell::new(contact.number.to_string(), ""),
                    ]
                })
                .collect(),
        ),
        Section::Groups => grid(
            "cps__two",
            &["Group list", "Contacts"],
            members(
                codeplug
                    .group_lists
                    .iter()
                    .map(|list| (list.name.clone(), list.contacts.clone())),
            ),
        ),
        Section::Scans => grid(
            "cps__two",
            &["Scan list", "Channels"],
            members(
                codeplug
                    .scan_lists
                    .iter()
                    .map(|list| (list.name.clone(), list.channels.clone())),
            ),
        ),
        Section::Ids => grid(
            "cps__two",
            &["Name", "DMR ID"],
            codeplug
                .radio_ids
                .iter()
                .map(|id| {
                    vec![
                        Cell::new(id.name.clone(), ""),
                        Cell::new(id.number.to_string(), ""),
                    ]
                })
                .collect(),
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub tone: &'static str,
    pub tip: Option<String>,
}

impl Cell {
    fn new(text: impl Into<String>, tone: &'static str) -> Self {
        Self {
            text: text.into(),
            tone,
            tip: None,
        }
    }
}

type Row = Vec<Cell>;

fn channel_rows(codeplug: &Codeplug) -> Vec<Row> {
    codeplug
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            vec![
                Cell::new((index + 1).to_string(), "tool-faint"),
                Cell::new(channel.name.clone(), ""),
                Cell::new(format_mhz(channel.rx_hz), ""),
                Cell::new(format_shift(channel), "tool-dim"),
                Cell::new(kind_label(channel_kind(channel)), ""),
                Cell::new(power_label(channel.power), "tool-dim"),
                Cell::new(channel_detail(channel), "tool-dim"),
                Cell::new(
                    channel.scan_list.clone().unwrap_or_else(|| "-".to_owned()),
                    "tool-dim",
                ),
            ]
        })
        .collect()
}

#[must_use]
pub fn member_line(members: &[String]) -> String {
    if members.is_empty() {
        "-".to_owned()
    } else {
        format!("{} \u{b7} {}", members.len(), members.join(", "))
    }
}

#[must_use]
pub fn member_cell(members: &[String]) -> Cell {
    Cell {
        tip: (!members.is_empty()).then(|| members.join(", ")),
        ..Cell::new(member_line(members), "tool-dim")
    }
}

fn members(rows: impl Iterator<Item = (String, Vec<String>)>) -> Vec<Row> {
    rows.map(|(name, members)| vec![Cell::new(name, ""), member_cell(&members)])
        .collect()
}

fn grid(columns: &'static str, head: &[&'static str], rows: Vec<Row>) -> impl IntoView {
    let mut cells: Vec<AnyView> = head
        .iter()
        .map(|title| AnyView::new(view! { text(class = "tool-th") {{*title}} }))
        .collect();
    for row in rows {
        cells.extend(row.into_iter().map(cell_view));
    }
    view! { box(class = "tool-table", class = columns) {{cells}} }
}

fn cell_view(cell: Cell) -> AnyView {
    let Cell { text, tone, tip } = cell;
    let class = match tone {
        "tool-dim" => "tool-td dim",
        "tool-faint" => "tool-td faint",
        _ => "tool-td",
    };
    match tip {
        Some(tip) => {
            let tip = Signal::stored(tip);
            AnyView::new(view! {
                Tooltip(delay = std::time::Duration::from_millis(250)) {
                    TooltipTrigger(class = class) { text {{text}} }
                    TooltipContent {{move || tip.get()}}
                }
            })
        }
        None => AnyView::new(view! { text(class = class) {{text}} }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_list_leads_with_its_count() {
        assert_eq!(member_line(&[]), "-");
        assert_eq!(
            member_line(&["A".to_owned(), "B".to_owned()]),
            "2 \u{b7} A, B"
        );
    }

    #[test]
    fn a_member_cell_carries_the_full_list_as_its_hint() {
        let names = ["A".to_owned(), "B".to_owned()];
        assert_eq!(member_cell(&names).tip.as_deref(), Some("A, B"));
        assert_eq!(member_cell(&[]).tip, None);
    }
}
