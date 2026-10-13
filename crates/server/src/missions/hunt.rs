use sdrmm_wire::{
    CONTROL_PORT, ChannelInfo, ChannelTarget, HuntMission, HuntNode, HuntStatus, IQ_PORT,
    MissionBody, MissionControl, MissionProblem, SweepState,
};

use super::{Built, Scene, TRIANGULATION, graph};

pub(super) fn hunt(scene: &Scene<'_>, node: &str, settings: &HuntNode) -> Built {
    let mut problems = Vec::new();
    let channel_node = graph::controlled_channel(&scene.resolved, node);
    let bound = channel_node
        .as_deref()
        .and_then(|channel| scene.channel(channel));
    let (position, phone) = graph::position_link(&scene.resolved, &scene.state.phones, node);
    let mut mission = HuntMission {
        target: None,
        status: None,
        clicks: settings.clicks,
        position,
        triangulations: graph::event_targets(&scene.resolved, node, TRIANGULATION),
    };
    let mut scanning = false;
    match (channel_node, bound) {
        (None, _) => problems.push(MissionProblem::Unwired {
            port: CONTROL_PORT.to_owned(),
        }),
        (Some(_), None) => problems.push(MissionProblem::NotRunning),
        (Some(channel_node), Some((set, info))) => {
            scanning = set
                .scanners
                .iter()
                .any(|scan| scan.settings.channel == info.id);
            if scanning {
                problems.push(MissionProblem::Scanning);
            }
            if info.out_of_band.is_some() {
                problems.push(MissionProblem::OutOfBand);
            }
            mission.status = set
                .hunts
                .iter()
                .find(|hunt| hunt.settings.channel == info.id)
                .cloned();
            mission.target = Some(target(set.id, info, channel_node));
        }
    }
    if scene.resolved.patch.first_feed(node, IQ_PORT).is_none() {
        problems.push(MissionProblem::Unwired {
            port: IQ_PORT.to_owned(),
        });
    }
    problems.extend(phone);
    let controls = controls(&mission, scanning);
    Built {
        body: MissionBody::Hunt(mission),
        problems,
        controls,
    }
}

fn target(device_set: u32, info: &ChannelInfo, channel_node: String) -> ChannelTarget {
    let (low, high) = sdrmm_channels::occupied_band(&info.settings.params);
    ChannelTarget {
        device_set,
        channel: info.id,
        channel_node,
        channel_type: info.settings.params.type_id().to_owned(),
        frequency_hz: info.settings.frequency_hz,
        bandwidth_hz: high - low,
    }
}

fn sweeping(status: Option<&HuntStatus>) -> bool {
    status
        .and_then(|status| status.sweep.as_ref())
        .is_some_and(|sweep| sweep.state != SweepState::Off)
}

fn controls(mission: &HuntMission, scanning: bool) -> Vec<MissionControl> {
    if mission.target.is_none() {
        return Vec::new();
    }
    let running = mission.status.is_some();
    let mut controls = vec![MissionControl::Tune];
    if running {
        controls.push(MissionControl::StopHunt);
    } else if !scanning {
        controls.push(MissionControl::StartHunt);
    }
    if mission.position.is_none() {
        return controls;
    }
    if sweeping(mission.status.as_ref()) {
        controls.push(MissionControl::StopSweep);
    } else if !scanning {
        controls.push(MissionControl::StartSweep);
    }
    if running {
        controls.push(MissionControl::Mark);
    }
    controls
}
