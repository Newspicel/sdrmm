pub(super) const TEXT_MESSAGE: u32 = 1;
pub(super) const POSITION: u32 = 3;
pub(super) const NODEINFO: u32 = 4;
pub(super) const ROUTING: u32 = 5;
pub(super) const WAYPOINT: u32 = 8;
pub(super) const TELEMETRY: u32 = 67;
pub(super) const TRACEROUTE: u32 = 70;
pub(super) const NEIGHBORINFO: u32 = 71;
pub(super) const MAP_REPORT: u32 = 73;

const PORT_NAMES: &[(u32, &str)] = &[
    (0, "UNKNOWN_APP"),
    (1, "TEXT_MESSAGE_APP"),
    (2, "REMOTE_HARDWARE_APP"),
    (3, "POSITION_APP"),
    (4, "NODEINFO_APP"),
    (5, "ROUTING_APP"),
    (6, "ADMIN_APP"),
    (7, "TEXT_MESSAGE_COMPRESSED_APP"),
    (8, "WAYPOINT_APP"),
    (9, "AUDIO_APP"),
    (10, "DETECTION_SENSOR_APP"),
    (11, "ALERT_APP"),
    (12, "KEY_VERIFICATION_APP"),
    (13, "REMOTE_SHELL_APP"),
    (32, "REPLY_APP"),
    (33, "IP_TUNNEL_APP"),
    (34, "PAXCOUNTER_APP"),
    (35, "STORE_FORWARD_PLUSPLUS_APP"),
    (36, "NODE_STATUS_APP"),
    (37, "MESH_BEACON_APP"),
    (38, "PAGING_APP"),
    (64, "SERIAL_APP"),
    (65, "STORE_FORWARD_APP"),
    (66, "RANGE_TEST_APP"),
    (67, "TELEMETRY_APP"),
    (68, "ZPS_APP"),
    (69, "SIMULATOR_APP"),
    (70, "TRACEROUTE_APP"),
    (71, "NEIGHBORINFO_APP"),
    (72, "ATAK_PLUGIN"),
    (73, "MAP_REPORT_APP"),
    (74, "POWERSTRESS_APP"),
    (75, "LORAWAN_BRIDGE"),
    (76, "RETICULUM_TUNNEL_APP"),
    (77, "CAYENNE_APP"),
    (78, "ATAK_PLUGIN_V2"),
    (79, "LORA_OTA_APP"),
    (112, "GROUPALARM_APP"),
    (256, "PRIVATE_APP"),
    (257, "ATAK_FORWARDER"),
];

const ROUTING_ERRORS: &[(u32, &str)] = &[
    (1, "NO_ROUTE"),
    (2, "GOT_NAK"),
    (3, "TIMEOUT"),
    (4, "NO_INTERFACE"),
    (5, "MAX_RETRANSMIT"),
    (6, "NO_CHANNEL"),
    (7, "TOO_LARGE"),
    (8, "NO_RESPONSE"),
    (9, "DUTY_CYCLE_LIMIT"),
    (32, "BAD_REQUEST"),
    (33, "NOT_AUTHORIZED"),
    (34, "PKI_FAILED"),
    (35, "PKI_UNKNOWN_PUBKEY"),
    (36, "ADMIN_BAD_SESSION_KEY"),
    (37, "ADMIN_PUBLIC_KEY_UNAUTHORIZED"),
    (38, "RATE_LIMIT_EXCEEDED"),
    (39, "PKI_SEND_FAIL_PUBLIC_KEY"),
];

fn lookup(table: &[(u32, &'static str)], code: u32) -> Option<&'static str> {
    table
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, name)| *name)
}

pub(super) fn port_name(port: u32) -> Option<&'static str> {
    lookup(PORT_NAMES, port)
}

pub(super) fn routing_error(code: u32) -> String {
    lookup(ROUTING_ERRORS, code).map_or_else(|| format!("ERROR_{code}"), str::to_owned)
}
