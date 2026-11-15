use super::EventFacts;

pub(super) const HEADER: &str =
    "at,kind,output,device_set,channel,freq_hz,station,summary,data\r\n";
const FORMULA_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

pub(super) fn row(output_node: &str, facts: &EventFacts) -> String {
    let data = facts
        .record
        .pointer("/event/data")
        .map(serde_json::Value::to_string)
        .unwrap_or_default();
    let fields = [
        field(&facts.at),
        field(&facts.kind),
        field(output_node),
        facts.device_set.to_string(),
        facts.channel.to_string(),
        facts.freq_hz.to_string(),
        field(facts.station.as_deref().unwrap_or_default()),
        field(&facts.summary),
        field(&data),
    ];
    let mut line = fields.join(",");
    line.push_str("\r\n");
    line
}

pub(crate) fn field(value: &str) -> String {
    let guarded = if value.starts_with(FORMULA_STARTS) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}
