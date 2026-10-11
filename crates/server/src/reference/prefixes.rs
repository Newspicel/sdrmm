use std::collections::HashMap;

use sdrmm_wire::Callsign;

const HEADER_COLONS: usize = 8;
const VERSION_MARK: &str = "VER";
const VERSION_DIGITS: usize = 8;
const WAE_ONLY: char = '*';
const CONTINENTS: [&str; 7] = ["AF", "AN", "AS", "EU", "NA", "OC", "SA"];
const PORTABLE: [&str; 7] = ["P", "M", "QRP", "A", "LH", "R", "B"];
const AT_SEA: [&str; 2] = ["MM", "AM"];
const MIN_CALL: usize = 3;
const MAX_CALL: usize = 20;
const MAX_SSID: usize = 2;

pub(crate) struct Prefixes {
    pub(crate) version: String,
    entities: Vec<Entity>,
    prefixes: HashMap<String, Zone>,
    calls: HashMap<String, Zone>,
}

struct Entity {
    country: String,
    prefix: String,
}

#[derive(Clone, Copy)]
struct Zone {
    entity: usize,
    cq: u8,
    itu: u8,
    continent: usize,
}

impl Prefixes {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(body).map_err(|_| "a prefix list that is not text")?;
        let mut table = Self {
            version: String::new(),
            entities: Vec::new(),
            prefixes: HashMap::new(),
            calls: HashMap::new(),
        };
        for record in text.split(';').map(str::trim).filter(|r| !r.is_empty()) {
            table.add(record)?;
        }
        if table.entities.is_empty() {
            return Err("a prefix list without countries".to_owned());
        }
        table.version = table
            .calls
            .keys()
            .filter_map(|call| call.strip_prefix(VERSION_MARK))
            .find(|digits| {
                digits.len() == VERSION_DIGITS && digits.bytes().all(|b| b.is_ascii_digit())
            })
            .ok_or("a prefix list without a version")?
            .to_owned();
        Ok(table)
    }

    pub(crate) fn lookup(&self, raw: &str) -> Option<Callsign> {
        let call = normalized(raw)?;
        let zone = match self.calls.get(&call) {
            Some(zone) => *zone,
            None => self.by_prefix(location(&call)?)?,
        };
        let entity = self.entities.get(zone.entity)?;
        Some(Callsign {
            call,
            country: entity.country.clone(),
            prefix: entity.prefix.clone(),
            continent: CONTINENTS.get(zone.continent)?.to_string(),
            cq_zone: zone.cq,
            itu_zone: zone.itu,
        })
    }

    fn by_prefix(&self, part: &str) -> Option<Zone> {
        (1..=part.len())
            .rev()
            .find_map(|len| part.get(..len).and_then(|prefix| self.prefixes.get(prefix)))
            .copied()
    }

    fn add(&mut self, record: &str) -> Result<(), String> {
        let broken = || {
            format!(
                "a broken country record: {}",
                record.lines().next().unwrap_or(record)
            )
        };
        let split = record
            .match_indices(':')
            .nth(HEADER_COLONS - 1)
            .ok_or_else(broken)?
            .0;
        let fields: Vec<&str> = record[..split].split(':').map(str::trim).collect();
        let [country, cq, itu, continent, _, _, _, prefix] = fields.as_slice() else {
            return Err(broken());
        };
        if prefix.starts_with(WAE_ONLY) {
            return Ok(());
        }
        let base = Zone {
            entity: self.entities.len(),
            cq: cq.parse().map_err(|_| broken())?,
            itu: itu.parse().map_err(|_| broken())?,
            continent: continent_index(continent).ok_or_else(broken)?,
        };
        self.entities.push(Entity {
            country: (*country).to_owned(),
            prefix: (*prefix).to_owned(),
        });
        for alias in record[split + 1..]
            .split(',')
            .map(str::trim)
            .filter(|a| !a.is_empty())
        {
            let (exact, name, zone) = parsed_alias(alias, base).ok_or_else(broken)?;
            let table = if exact {
                &mut self.calls
            } else {
                &mut self.prefixes
            };
            table.insert(name, zone);
        }
        Ok(())
    }
}

fn parsed_alias(alias: &str, base: Zone) -> Option<(bool, String, Zone)> {
    let (exact, rest) = alias
        .strip_prefix('=')
        .map_or((false, alias), |rest| (true, rest));
    let end = rest.find(['(', '[', '<', '{', '~']).unwrap_or(rest.len());
    let name = rest.get(..end).filter(|name| !name.is_empty())?.to_owned();
    let mut zone = base;
    if let Some(cq) = between(rest, '(', ')') {
        zone.cq = cq.parse().ok()?;
    }
    if let Some(itu) = between(rest, '[', ']') {
        zone.itu = itu.parse().ok()?;
    }
    if let Some(continent) = between(rest, '{', '}') {
        zone.continent = continent_index(continent)?;
    }
    Some((exact, name, zone))
}

fn between(text: &str, open: char, close: char) -> Option<&str> {
    let start = text.find(open)? + open.len_utf8();
    let length = text.get(start..)?.find(close)?;
    text.get(start..start + length)
}

fn continent_index(code: &str) -> Option<usize> {
    CONTINENTS.iter().position(|known| *known == code)
}

fn normalized(raw: &str) -> Option<String> {
    let token = raw.split_whitespace().next()?;
    let mut call = token
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_uppercase();
    if let Some((base, ssid)) = call.rsplit_once('-')
        && (1..=MAX_SSID).contains(&ssid.len())
        && ssid.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        call.truncate(base.len());
    }
    let shaped = (MIN_CALL..=MAX_CALL).contains(&call.len())
        && call.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'/')
        && call.bytes().any(|b| b.is_ascii_digit())
        && call.bytes().any(|b| b.is_ascii_alphabetic());
    shaped.then_some(call)
}

fn location(call: &str) -> Option<&str> {
    let parts: Vec<&str> = call.split('/').filter(|part| !part.is_empty()).collect();
    if parts.iter().any(|part| AT_SEA.contains(part)) {
        return None;
    }
    parts
        .into_iter()
        .filter(|part| !PORTABLE.contains(part))
        .filter(|part| !(part.len() == 1 && part.bytes().all(|b| b.is_ascii_digit())))
        .min_by_key(|part| part.len())
}

#[cfg(test)]
mod tests;
