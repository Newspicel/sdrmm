pub fn word_after(text: &str, marker: &str) -> Option<String> {
    let (_, rest) = text.split_once(marker)?;
    let word = rest
        .split_whitespace()
        .next()?
        .trim_matches(|c: char| c == ',' || c == '|');
    (!word.is_empty()).then(|| word.to_owned())
}

pub fn strip_ansi(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for end in chars.by_ref() {
                if end.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    plain
}

pub fn mode_s(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix('*')?.strip_suffix(';'))
        .filter(|hex| !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_ascii_uppercase)
        .collect()
}

pub fn nmea_payloads(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.starts_with("!AIVDM") || line.starts_with("!AIVDO"))
        .filter_map(|line| line.split(',').nth(5))
        .map(str::to_owned)
        .collect()
}

pub fn direwolf(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix('[')?;
            let (channel, packet) = rest.split_once("] ")?;
            let numbered = channel.chars().all(|c| c.is_ascii_digit() || c == '.');
            (numbered && packet.contains('>') && packet.contains(':')).then(|| packet.to_owned())
        })
        .collect()
}

pub fn multimon_aprs(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("APRS: "))
        .map(str::to_owned)
        .collect()
}

pub fn multimon_pocsag(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.starts_with("POCSAG") && line.contains("Address:"))
        .map(|line| line.replace("<NUL>", "").trim_end().to_owned())
        .collect()
}

pub fn acarsdec(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .map(|message| {
            let field = |name: &str| message[name].as_str().unwrap_or_default().to_owned();
            format!("{} {} {}", field("tail"), field("label"), field("text"))
        })
        .collect()
}

pub fn ft8_lib(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|stamp| stamp.len() == 6 && stamp.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|line| line.split_once('~'))
        .map(|(_, message)| message.trim().to_owned())
        .collect()
}

pub fn dsd_fme(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .filter(|line| line.contains("TGT=") && line.contains("SRC="))
        .map(|line| line.trim().to_owned())
        .collect()
}

pub fn multimon_flex(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.strip_prefix("FLEX_NEXT|")?.split('|').collect();
            let capcode = fields.get(3)?;
            let message = fields.last()?;
            Some(format!("{capcode} {message}"))
        })
        .collect()
}

pub fn multimon_ccir(text: &str) -> Vec<String> {
    multimon_selcall(text, "CCIR: ")
}

pub fn multimon_zvei(text: &str) -> Vec<String> {
    multimon_selcall(text, "ZVEI1: ")
}

/// Selcall codes with multimon-ng's repeat tone, printed as `E`, read as the digit it repeats.
fn multimon_selcall(text: &str, prefix: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix(prefix))
        .map(|code| {
            let mut previous = None;
            code.trim()
                .chars()
                .map(|symbol| {
                    let digit = match (symbol, previous) {
                        ('E', Some(repeated)) => repeated,
                        _ => symbol,
                    };
                    previous = Some(digit);
                    digit
                })
                .collect()
        })
        .collect()
}

pub fn dump978(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix('-')?.split(';').next())
        .filter(|hex| !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_ascii_uppercase)
        .collect()
}

pub fn words(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.starts_with("###"))
        .flat_map(str::split_whitespace)
        .map(str::to_owned)
        .collect()
}

pub fn rtl_433(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|event| {
            let model = event["model"].as_str()?;
            let id = match &event["id"] {
                serde_json::Value::String(id) => id.clone(),
                serde_json::Value::Number(id) => id.to_string(),
                _ => return None,
            };
            Some(format!("{model} {id}"))
        })
        .collect()
}

pub fn redsea(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .flat_map(|group| {
            let field = |name: &str| group[name].as_str().map(str::trim).map(str::to_owned);
            [
                field("pi").map(|pi| format!("PI {}", pi.trim_start_matches("0x"))),
                field("ps").map(|ps| format!("PS {ps}")),
                field("radiotext").map(|text| format!("RT {text}")),
            ]
        })
        .flatten()
        .collect()
}

pub fn dsd_fme_nxdn(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .filter(|line| line.contains("Src=") && line.contains("Dst/TG="))
        .map(|line| line.trim().to_owned())
        .collect()
}

pub fn dsd_fme_source(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .filter_map(|line| word_after(line, "SRC:"))
        .collect()
}

pub fn dsd_fme_p25(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .filter_map(|line| line.split_once("Group Voice Channel User - Group "))
        .map(|(_, call)| call.trim().to_owned())
        .collect()
}

pub fn dsd_fme_dpmr(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .filter(|line| line.contains("TG=") && line.contains("Src=") && !line.contains("CRC ERR"))
        .map(|line| line.trim().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_the_word_after_the_marker() {
        let banner = "| dump1090 ModeS Receiver      dump1090-fa 11.1 |";
        assert_eq!(word_after(banner, "dump1090-fa ").as_deref(), Some("11.1"));
        let readsb = "readsb version: 3.16.17 compiled on 260927";
        assert_eq!(word_after(readsb, "version:").as_deref(), Some("3.16.17"));
        let direwolf = "Dire Wolf Release 1.8.1, November 2025";
        assert_eq!(word_after(direwolf, "Release").as_deref(), Some("1.8.1"));
        assert_eq!(word_after("nothing", "Release"), None);
    }

    #[test]
    fn mode_s_keeps_hex_frames() {
        let out = "starting up\n*8d3ff91d99144e0c18080a682221;\n*5d780561d30828;\n*zz;\n";
        assert_eq!(
            mode_s(out),
            ["8D3FF91D99144E0C18080A682221", "5D780561D30828"]
        );
    }

    #[test]
    fn nmea_payloads_ignore_the_channel() {
        let out = "!AIVDM,1,1,,A,139Lg00P,0*3F\n!AIVDM,1,1,,B,139Lg00P,0*3C\nbanner";
        assert_eq!(nmea_payloads(out), ["139Lg00P", "139Lg00P"]);
    }

    #[test]
    fn direwolf_packets_lose_colour_and_channel() {
        let out = "\u{1b}[38;2;0;192;0m[0] DL1ABC-9>APRS,WIDE1-1:!5230.00N/01324.00E>x\n\
                   \u{1b}[38;2;0;0;0m\n[0.3] A>B:c\n1 packets decoded in 0.017 seconds.";
        assert_eq!(
            direwolf(out),
            ["DL1ABC-9>APRS,WIDE1-1:!5230.00N/01324.00E>x", "A>B:c"]
        );
    }

    #[test]
    fn multimon_lines_are_read_per_mode() {
        let out = "APRS: DL1ABC-9>APRS:!x\nPOCSAG1200: Address: 1234567  Function: 3  Alpha:   SDRmm FIXTURE<NUL>\nAFSK1200: fm";
        assert_eq!(multimon_aprs(out), ["DL1ABC-9>APRS:!x"]);
        assert_eq!(
            multimon_pocsag(out),
            ["POCSAG1200: Address: 1234567  Function: 3  Alpha:   SDRmm FIXTURE"]
        );
    }

    #[test]
    fn acarsdec_json_becomes_tail_label_text() {
        let out = "{\"tail\":\"F-GTAE\",\"label\":\"H1\",\"text\":\"#DFB\"}\n\
                   {\"tail\":\"LN-DYY\",\"label\":\"_d\"}\nexiting ...";
        assert_eq!(acarsdec(out), ["F-GTAE H1 #DFB", "LN-DYY _d "]);
    }

    #[test]
    fn ft8_lib_decodes_are_the_text_after_the_tilde() {
        let out = "Decoded 2 messages\n000000 +19.0 +1.52  906 ~  PA3EPP SP8NFO KN09\n\
                   000000 +10.5 +1.68  297 ~  <...> ON7EE JO10\n";
        assert_eq!(ft8_lib(out), ["PA3EPP SP8NFO KN09", "<...> ON7EE JO10"]);
    }

    #[test]
    fn dsd_fme_counts_frames_with_addressing() {
        let out = "Sync: VLC \u{1b}[33m\n SLOT 1 Unknown LC\n\
                   \u{1b}[32m SLOT 1 TGT=12345678 SRC=12345678 Group Call \u{1b}[0m\n";
        assert_eq!(
            dsd_fme(out),
            ["SLOT 1 TGT=12345678 SRC=12345678 Group Call"]
        );
    }

    #[test]
    fn multimon_flex_keys_capcode_and_text() {
        let out = "FLEX_NEXT|2026-10-06 19:07:20+02:00|1600/2|00.072.A|0002029574|T|ALN|K.0/3|A2 DP2 Voorburg\nFLEX: noise\n";
        assert_eq!(multimon_flex(out), ["0002029574 A2 DP2 Voorburg"]);
    }

    #[test]
    fn multimon_selcall_reads_the_repeat_tone_as_its_digit() {
        let out = "CCIR: 12E34\nZVEI1: A1ED0\nZVEI1: E1\n";
        assert_eq!(multimon_ccir(out), ["12234"]);
        assert_eq!(multimon_zvei(out), ["A11D0", "E1"]);
    }

    #[test]
    fn dump978_keeps_downlink_frames() {
        let out = "-00a66ef135445d52;rssi=0.1;t=0.002;\n+3514c952;rssi=0.1;t=0.009;\n\
                   -0aaba4be38e5;rs=2;rssi=0.0;t=0.022;\nMessage source reports EOF\n";
        assert_eq!(dump978(out), ["00A66EF135445D52", "0AABA4BE38E5"]);
    }

    #[test]
    fn words_skip_status_lines() {
        let out = "### CARRIER 45.45 @ 1590.0 Hz ###\nCQ CQ DE DL1ABC K\r\n\n\
                   ### NOCARRIER ndata=21 ###\n";
        assert_eq!(words(out), ["CQ", "CQ", "DE", "DL1ABC", "K"]);
    }

    #[test]
    fn rtl_433_keys_model_and_id() {
        let out = "{\"time\" : \"@0.01s\", \"model\" : \"Acurite-609TXC\", \"id\" : 162}\n\
                   {\"model\" : \"Schrader\", \"type\" : \"TPMS\", \"id\" : \"03A38B2\"}\n\
                   {\"model\" : \"FS20\", \"housecode\" : 286331153}\nbanner\n";
        assert_eq!(rtl_433(out), ["Acurite-609TXC 162", "Schrader 03A38B2"]);
    }

    #[test]
    fn redsea_groups_give_pi_ps_and_radiotext() {
        let out = "{\"pi\":\"0xD3C2\",\"group\":\"0A\",\"ps\":\"SDR-M4  \"}\n\
                   {\"pi\":\"0xD3C2\",\"group\":\"2A\",\"radiotext\":\"SDRmm reference fixture\"}\n\
                   {\"pi\":\"0xD3C2\",\"group\":\"0A\",\"partial_ps\":\"SDR\"}\n";
        assert_eq!(
            redsea(out),
            [
                "PI D3C2",
                "PS SDR-M4",
                "PI D3C2",
                "RT SDRmm reference fixture",
                "PI D3C2"
            ]
        );
    }

    #[test]
    fn dsd_fme_nxdn_keeps_addressed_lines() {
        let out = "Sync: NXDN48 RTCH Voice\n Broadcast Call - Src=12345 - Dst/TG=234 \n";
        assert_eq!(
            dsd_fme_nxdn(out),
            ["Broadcast Call - Src=12345 - Dst/TG=234"]
        );
    }

    #[test]
    fn dsd_fme_keeps_source_calls() {
        let out = "Sync: +YSF FN: 2/6 SRC: DL1ABC    \nSync: +YSF FN: 1/6 DST: ALL SRC: DL1ABC U/L: DB0ABC\n";
        assert_eq!(dsd_fme_source(out), ["DL1ABC", "DL1ABC"]);
    }

    #[test]
    fn dsd_fme_p25_keeps_the_group_and_source() {
        let out = "Sync: +P25p1 LDU1  Group Voice Channel User - Group 4609 Source 11259375 \nSync: +P25p1 LDU2\n";
        assert_eq!(dsd_fme_p25(out), ["4609 Source 11259375"]);
    }
}
