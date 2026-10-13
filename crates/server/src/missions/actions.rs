use sdrmm_wire::{
    HuntAction, HuntMission, Mission, MissionAction, MissionBody, NodeAction, SurveyAction,
};

use super::{MissionRefusal, mission};
use crate::{
    AppState, array, df_fusion,
    rest::{self, AppError},
};

pub(crate) fn act(
    state: &AppState,
    node: &str,
    action: MissionAction,
    author: Option<&str>,
) -> Result<Mission, AppError> {
    let mission = current(state, node)?;
    if !mission.controls.contains(&action.control()) {
        return Err(MissionRefusal::NotAControl.into());
    }
    dispatch(state, &mission, action, author)?;
    current(state, node)
}

fn current(state: &AppState, node: &str) -> Result<Mission, AppError> {
    Ok(mission(state, node)?
        .ok_or_else(|| MissionRefusal::NoMission(node.to_owned()))?
        .1)
}

fn dispatch(
    state: &AppState,
    mission: &Mission,
    action: MissionAction,
    author: Option<&str>,
) -> Result<(), AppError> {
    match &mission.body {
        MissionBody::Hunt(hunt) => hunt_action(state, (&mission.node, hunt), action, author),
        MissionBody::Df(df) => array_action(state, df.array.as_deref(), action),
        MissionBody::Radar(radar) => array_action(state, radar.array.as_deref(), action),
        MissionBody::Survey(_) => survey_action(state, &mission.node, action),
        MissionBody::Triangulation(_) => match action {
            MissionAction::ClearFusion => df_fusion::clear(state, &mission.node),
            _ => Err(MissionRefusal::NotAControl.into()),
        },
    }
}

fn hunt_action(
    state: &AppState,
    (node, hunt): (&str, &HuntMission),
    action: MissionAction,
    author: Option<&str>,
) -> Result<(), AppError> {
    let target = hunt.target.as_ref().ok_or(MissionRefusal::NotRunning)?;
    let action = match action {
        MissionAction::Tune { frequency_hz } => {
            let tuner = rest::Tuner {
                author,
                actor: Some(node),
            };
            return rest::tune_node(state, &target.channel_node, positive(frequency_hz)?, tuner);
        }
        MissionAction::StartHunt | MissionAction::StopSweep => HuntAction::Start,
        MissionAction::StopHunt => HuntAction::Stop,
        MissionAction::StartSweep => HuntAction::Sweep,
        MissionAction::Mark => HuntAction::Mark,
        _ => return Err(MissionRefusal::NotAControl.into()),
    };
    rest::controlled(rest::act(state, node, NodeAction::Hunt(action), None)?)?;
    Ok(())
}

fn tune(
    state: &AppState,
    node: &str,
    frequency_hz: f64,
    author: Option<&str>,
) -> Result<(), AppError> {
    let freq_hz = positive(frequency_hz)?;
    rest::act(state, node, NodeAction::Tune { freq_hz }, author)?;
    Ok(())
}

fn array_action(
    state: &AppState,
    array: Option<&str>,
    action: MissionAction,
) -> Result<(), AppError> {
    let array = array.ok_or(MissionRefusal::NotRunning)?;
    match action {
        MissionAction::Tune { frequency_hz } => tune(state, array, frequency_hz, None),
        MissionAction::Calibrate => array::calibrate(state, array),
        _ => Err(MissionRefusal::NotAControl.into()),
    }
}

fn survey_action(state: &AppState, node: &str, action: MissionAction) -> Result<(), AppError> {
    let survey = match action {
        MissionAction::StartSurvey => SurveyAction::Start,
        MissionAction::StopSurvey => SurveyAction::Stop,
        MissionAction::ClearSurvey => SurveyAction::Clear,
        _ => return Err(MissionRefusal::NotAControl.into()),
    };
    state.survey.act(state, node, survey)?;
    Ok(())
}

fn positive(frequency_hz: f64) -> Result<f64, MissionRefusal> {
    if frequency_hz.is_finite() && frequency_hz > 0.0 {
        Ok(frequency_hz)
    } else {
        Err(MissionRefusal::Frequency)
    }
}
