use zgui::prelude::*;

use crate::ui::faces::processor;

pub fn install() {
    install_stylesheet("processor-face", processor::SHEET);
    install_stylesheet("array-face", SHEET);
}

const SHEET: &str = css!(
    r#"
.af { gap: 8px; }
.af__reads { flex-wrap: wrap; row-gap: 2px; }
.af__read { width: 50%; gap: 6px; align-items: baseline; min-width: 0; }
.af__read_name {
    flex: 0 0 auto;
    width: 56px;
    font-family: var(--mono);
    font-size: 9px;
    letter-spacing: 0.09em;
    text-transform: uppercase;
    color: var(--ink-faint);
}
.af__read_value { flex: 1 1 auto; min-width: 0; font-family: var(--mono); font-size: 11px; color: var(--ink); }
.af__read_value.danger { color: var(--danger); }
.af__stats { flex-wrap: wrap; gap: 6px; flex: 1 1 auto; min-width: 0; }
.pf__stat.af__plain { border-color: var(--line); color: var(--ink-dim); }
.af__danger .pick { color: var(--danger); }
.af__lanes { gap: 2px; padding-top: 6px; border-top: 1px solid var(--line); }
.af__lane_list { gap: 1px; }
.af__lane { align-items: center; gap: 6px; min-height: 18px; }
.af__lane_name { flex: 0 0 auto; width: 22px; font-family: var(--mono); font-size: 11px; color: oklch(0.78 0.12 160); }
.af__lane_source {
    flex: 0 1 96px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: 10px;
    color: var(--ink-faint);
}
.af__bar { flex: 1 1 auto; min-width: 24px; height: 6px; border-radius: 3px; background: var(--well); overflow: hidden; }
.af__bar_fill { height: 6px; background: oklch(0.78 0.12 160); }
.af__lane_gain { flex: 0 0 auto; width: 58px; text-align: right; font-family: var(--mono); font-size: 11px; color: var(--ink); }
.af__lane_gain.danger { color: var(--danger); }
.af__lane_phase { flex: 0 0 auto; width: 34px; text-align: right; font-family: var(--mono); font-size: 11px; color: var(--ink-dim); }
.af__geo { gap: 8px; }
.af__geo_top { gap: 10px; align-items: flex-start; }
.af__geo_rows { flex: 1 1 auto; min-width: 0; }
.af__preview {
    position: relative;
    flex: 0 0 auto;
    width: 96px;
    height: 96px;
    border: 1px solid var(--line);
    border-radius: 3px;
    background: var(--well);
}
.af__preview_draw { position: absolute; left: 0; top: 0; width: 96px; height: 96px; }
.af__preview_lane {
    position: absolute;
    width: 10px;
    height: 10px;
    text-align: center;
    font-family: var(--mono);
    font-size: 6px;
    line-height: 10px;
    color: var(--ink);
}
.af__preview_north { position: absolute; width: 8px; font-family: var(--mono); font-size: 8px; color: var(--accent); }
.af__positions { gap: 2px; }
.af__pos { align-items: center; gap: 4px; }
.af__pos.af__unwired { opacity: 0.5; }
.af__pos.danger { color: var(--danger); }
.af__pos_lane { flex: 0 0 auto; width: 32px; font-family: var(--mono); font-size: 11px; }
.af__pos_head { flex: 1 1 0; font-family: var(--mono); font-size: 9px; letter-spacing: 0.09em; text-transform: uppercase; color: var(--ink-faint); }
"#
);
