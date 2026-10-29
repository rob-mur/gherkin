use std::{fs, path::Path};

use gherkin::{EnvError, Feature, GherkinEnv, ParseFileError};

fn load_feature<P: AsRef<Path>>(path: P) -> Result<Feature, ParseFileError> {
    Feature::parse_path(path.as_ref(), Default::default())
}

fn unknown_keyword(error: ParseFileError) -> String {
    match error {
        ParseFileError::Parsing {
            error: Some(EnvError::UnknownKeyword(s)),
            ..
        } => s,
        e => panic!("Invalid error: {:?}", e),
    }
}

#[test]
fn inconsistent_cell_count() {
    let error =
        load_feature("./tests/fixtures/data/bad/inconsistent_cell_count.feature").unwrap_err();
    match error {
        ParseFileError::Reading { .. } => panic!("Invalid error"),
        ParseFileError::Parsing { error, .. } => {
            let error = error.unwrap();
            match error {
                gherkin::EnvError::InconsistentCellCount(_) => {}
                _ => panic!("Invalid error"),
            }
        }
    };
}

#[test]
fn invalid_language() {
    let _ = load_feature("./tests/fixtures/data/bad/invalid_language.feature").unwrap_err();
}

#[test]
fn multiple_parser_errors() {
    let _ = load_feature("./tests/fixtures/data/bad/multiple_parser_errors.feature").unwrap_err();
}

#[test]
fn not_gherkin() {
    let error = load_feature("./tests/fixtures/data/bad/not_gherkin.feature").unwrap_err();
    assert_eq!(unknown_keyword(error), "not gherkin");
}

#[test]
fn single_parser_error() {
    let error = load_feature("./tests/fixtures/data/bad/single_parser_error.feature").unwrap_err();
    assert_eq!(unknown_keyword(error), "invalid line here");
}

#[test]
fn unexpected_eof() {
    let _ = load_feature("./tests/fixtures/data/bad/unexpected_eof.feature").unwrap_err();
}

#[test]
fn whitespace_in_tags() {
    let _ = load_feature("./tests/fixtures/data/bad/whitespace_in_tags.feature").unwrap_err();
}

#[test]
fn error_positions_match_reference() {
    for entry in fs::read_dir("./tests/fixtures/data/bad").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "feature") {
            continue;
        }

        // The first error the reference parser reports for the file. It omits
        // the `column` when the file ends unexpectedly, and prints it as 0.
        let errors = fs::read_to_string(path.with_extension("feature.errors.ndjson")).unwrap();
        let first: serde_json::Value =
            serde_json::from_str(errors.lines().next().unwrap()).unwrap();
        let location = &first["parseError"]["source"]["location"];
        let line = location["line"].as_u64().unwrap();
        let col = location["column"].as_u64().unwrap_or(0);
        let expected = format!("Error at {line}:{col}: ");

        let ParseFileError::Parsing { source, .. } = load_feature(&path).unwrap_err() else {
            panic!("{}: invalid error", path.display());
        };
        let source = source.to_string();
        assert!(
            source.starts_with(&expected),
            "{}: {source}",
            path.display()
        );

        let input = fs::read_to_string(&path).unwrap();
        let error = Feature::parse(input, GherkinEnv::default())
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(&expected), "{}: {error}", path.display());
    }
}
