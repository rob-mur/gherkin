use gherkin::{Feature, GherkinEnv};

#[test]
fn parse_comment_only() {
    let feature = Feature::parse("# only a comment", GherkinEnv::default()).unwrap();
    assert_eq!(feature, None);
}

#[test]
fn parse_path_empty() {
    let feature = Feature::parse_path(
        "./tests/fixtures/data/good/empty.feature",
        GherkinEnv::default(),
    )
    .unwrap();
    assert_eq!(feature, None);
}

#[test]
fn parse_path_comment_only() {
    let feature = Feature::parse_path(
        "./tests/fixtures/data/good/incomplete_feature_3.feature",
        GherkinEnv::default(),
    )
    .unwrap();
    assert_eq!(feature, None);
}

#[test]
fn parse_path_sets_path() {
    let feature = Feature::parse_path(
        "./tests/fixtures/data/good/minimal.feature",
        GherkinEnv::default(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        feature.path.as_deref(),
        Some("./tests/fixtures/data/good/minimal.feature".as_ref()),
    );
}
