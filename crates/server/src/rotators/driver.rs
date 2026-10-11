use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        mpsc::{self, RecvTimeoutError},
    },
    time::Duration,
};

use sdrmm_engine::Engine;
use sdrmm_wire::{RotatorMode, RotatorStatus, SatelliteStatus, ServerEvent, SkyPoint};

use super::{
    Connection, Follow,
    link::{self, Link, LinkError},
    locked,
    plan::{self, Mount, Plan},
};
use crate::{
    AppState,
    satellites::{SatelliteHub, unix_now},
};

const POLL: Duration = Duration::from_secs(1);
const RETRY: Duration = Duration::from_secs(5);
const UNWIRED: &str = "wire a satellite";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Aim {
    pub(super) target: SkyPoint,
    pub(super) awaiting_pass: bool,
    pub(super) flipped: bool,
}

impl Aim {
    const fn fixed(target: SkyPoint) -> Self {
        Self {
            target,
            awaiting_pass: false,
            flipped: false,
        }
    }
}

pub(super) struct Driver {
    node: String,
    connection: Connection,
    follow: Arc<Mutex<Follow>>,
    statuses: Arc<Mutex<HashMap<String, RotatorStatus>>>,
    engine: Weak<Engine>,
    satellites: Weak<SatelliteHub>,
    sent: Option<SkyPoint>,
    stopped: bool,
    plan: Option<Plan>,
}

impl Driver {
    pub(super) fn new(
        node: String,
        connection: Connection,
        follow: Arc<Mutex<Follow>>,
        statuses: Arc<Mutex<HashMap<String, RotatorStatus>>>,
        state: &AppState,
    ) -> Self {
        Self {
            node,
            connection,
            follow,
            statuses,
            engine: Arc::downgrade(&state.engine),
            satellites: Arc::downgrade(&state.satellites),
            sent: None,
            stopped: false,
            plan: None,
        }
    }

    pub(super) fn run(mut self, woken: &mpsc::Receiver<()>) {
        self.serve(woken);
        locked(&self.statuses).remove(&self.node);
    }

    fn serve(&mut self, woken: &mpsc::Receiver<()>) {
        loop {
            let error = match link::open(&self.connection.link, self.connection.axes) {
                Ok(mut link) => match self.drive(link.as_mut(), woken) {
                    Ok(()) => return,
                    Err(error) => error,
                },
                Err(error) => error,
            };
            self.publish(RotatorStatus {
                node: self.node.clone(),
                error: Some(error.to_string()),
                ..RotatorStatus::default()
            });
            if !wait(woken, RETRY) {
                return;
            }
        }
    }

    fn drive(&mut self, link: &mut dyn Link, woken: &mpsc::Receiver<()>) -> Result<(), LinkError> {
        self.sent = None;
        self.stopped = false;
        loop {
            let position = link.position()?;
            let follow = locked(&self.follow).clone();
            let aim = self.steer(link, &follow, position)?;
            self.publish(RotatorStatus {
                node: self.node.clone(),
                position: Some(position),
                target: aim.map(|aim| aim.target),
                awaiting_pass: aim.is_some_and(|aim| aim.awaiting_pass),
                flipped: aim.is_some_and(|aim| aim.flipped),
                error: (follow.settings.mode == RotatorMode::Track && follow.satellite.is_none())
                    .then(|| UNWIRED.to_owned()),
            });
            if !wait(woken, POLL) {
                return Ok(());
            }
        }
    }

    fn steer(
        &mut self,
        link: &mut dyn Link,
        follow: &Follow,
        position: SkyPoint,
    ) -> Result<Option<Aim>, LinkError> {
        let mount = Mount::of(&follow.settings);
        let aim = match follow.settings.mode {
            RotatorMode::Stop => {
                if !self.stopped {
                    link.stop()?;
                    self.stopped = true;
                }
                self.sent = None;
                return Ok(None);
            }
            RotatorMode::Manual => Some(Aim::fixed(mount.reach(follow.settings.manual))),
            RotatorMode::Park => Some(Aim::fixed(mount.reach(follow.settings.park))),
            RotatorMode::Track => self.track(follow, mount, position),
        };
        self.stopped = false;
        if let Some(aim) = aim
            && needs_move(self.sent, aim.target, follow.settings.tolerance_deg)
        {
            link.point(aim.target)?;
            self.sent = Some(aim.target);
        }
        Ok(aim)
    }

    fn track(&mut self, follow: &Follow, mount: Mount, position: SkyPoint) -> Option<Aim> {
        let satellite = follow.satellite.as_deref()?;
        let status = self.satellites.upgrade()?.status(satellite)?;
        self.replan(&status, mount, position.azimuth_deg);
        aim(
            &status,
            self.plan.as_ref(),
            mount,
            position,
            unix_now() as i64,
        )
    }

    fn replan(&mut self, status: &SatelliteStatus, mount: Mount, from_deg: f64) {
        let Some(pass) = status.next_pass else {
            self.plan = None;
            return;
        };
        if self
            .plan
            .as_ref()
            .is_some_and(|plan| plan.serves(mount, pass.max_at))
        {
            return;
        }
        self.plan = plan::plan(&status.pass_track, pass.max_at, mount, from_deg);
    }

    fn publish(&self, status: RotatorStatus) {
        locked(&self.statuses).insert(self.node.clone(), status.clone());
        if let Some(engine) = self.engine.upgrade() {
            engine.emit_event(ServerEvent::RotatorUpdate {
                status: Box::new(status),
            });
        }
    }
}

pub(super) fn aim(
    status: &SatelliteStatus,
    plan: Option<&Plan>,
    mount: Mount,
    position: SkyPoint,
    now: i64,
) -> Option<Aim> {
    let look = status.look?;
    let (sky, at, awaiting_pass) = if status.visible {
        let sky = SkyPoint {
            azimuth_deg: look.azimuth_deg,
            elevation_deg: look.elevation_deg.max(0.0),
        };
        (sky, now, false)
    } else {
        let rise = status.pass_track.first()?;
        (rise.sky(), rise.at, true)
    };
    let (target, flipped) = plan::steer(sky, at, plan, mount, position.azimuth_deg);
    Some(Aim {
        target,
        awaiting_pass,
        flipped,
    })
}

pub(super) fn needs_move(sent: Option<SkyPoint>, target: SkyPoint, tolerance_deg: f64) -> bool {
    sent.is_none_or(|sent| sent.off_by(target) >= tolerance_deg)
}

fn wait(woken: &mpsc::Receiver<()>, delay: Duration) -> bool {
    !matches!(
        woken.recv_timeout(delay),
        Err(RecvTimeoutError::Disconnected)
    )
}
