use num_complex::Complex;
use sdrmm_channels::synth::{
    decoder_fixtures::recipe,
    ident_fixtures::{Expect, FIXTURES, Fixture, identify, judge, samples},
};

fn fixture_iq(fixture: &Fixture) -> Vec<Complex<f32>> {
    if let Some(recipe) = recipe(fixture.stem) {
        return (recipe.iq)();
    }
    let file = fixture.file();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");
    samples(&std::fs::read(format!("{path}{file}")).unwrap_or_else(|e| panic!("{file}: {e}")))
}

#[test]
fn every_recorded_fixture_is_named_as_recorded() {
    let mut failures = Vec::new();
    for fixture in FIXTURES {
        let outcome = identify(fixture, &fixture_iq(fixture));
        if let Err(miss) = judge(fixture, &outcome) {
            failures.push(miss);
        }
    }
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
