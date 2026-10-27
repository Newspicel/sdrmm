use num_complex::Complex;
use sdrmm_channels::synth::{
    decoder_fixtures::recipe,
    ident_fixtures::{Expect, FIXTURES, Fixture, identify, judge, samples},
};

fn is_recorded(fixture: &Fixture) -> bool {
    recipe(fixture.stem).is_none()
}

fn is_synthesized(fixture: &Fixture) -> bool {
    recipe(fixture.stem).is_some()
}

fn recorded_iq(fixture: &Fixture) -> Vec<Complex<f32>> {
    let file = fixture.file();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");
    samples(&std::fs::read(format!("{path}{file}")).unwrap_or_else(|e| panic!("{file}: {e}")))
}

fn synthesized_iq(fixture: &Fixture) -> Vec<Complex<f32>> {
    let Some(recipe) = recipe(fixture.stem) else {
        panic!("{}: no recipe", fixture.stem)
    };
    (recipe.iq)()
}

fn judge_all(
    wanted: fn(&Fixture) -> bool,
    iq: fn(&Fixture) -> Vec<Complex<f32>>,
) -> (usize, Vec<String>) {
    let mut judged = 0;
    let mut failures = Vec::new();
    for fixture in FIXTURES.iter().filter(|fixture| wanted(fixture)) {
        judged += 1;
        if let Err(miss) = judge(fixture, &identify(fixture, &iq(fixture))) {
            failures.push(miss);
        }
    }
    (judged, failures)
}

#[test]
fn every_recorded_fixture_is_named_as_recorded() {
    let (judged, failures) = judge_all(is_recorded, recorded_iq);
    assert!(judged > 0, "no fixture reads a committed capture");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_synthesized_fixture_is_named_as_recorded() {
    let (judged, failures) = judge_all(is_synthesized, synthesized_iq);
    assert!(judged > 0, "no fixture comes from a recipe");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn what_lies_beyond_reach_says_why() {
    for fixture in FIXTURES {
        if let Expect::Beyond(why) = &fixture.expect {
            assert!(!why.is_empty(), "{}", fixture.stem);
        }
    }
}
