//! Test-only proof that Mecojoni's generic formatter boundary composes with
//! genuine Fluent resources. This is intentionally not a production adapter.

use std::{fs, path::PathBuf};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use mecojoni_core::{
    DataBinding, Diagnostic, DiagnosticCode, Formatter, FormatterRequest, FormatterResult,
    GenerationRequest, LocaleRequest, MecoError, MecoResult, MessageArgument, MessageDefinition,
    MessageManifest, PackageInput, PackageSource, Rational, SchemaType, Severity, SourceFile,
    SourceId, Value, compile_package_with_manifest,
};

type Bundle = FluentBundle<FluentResource>;

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/fluent")
}

fn read_manifest(directory: &std::path::Path) -> MessageManifest {
    let source = fs::read_to_string(directory.join("messages.manifest"))
        .expect("read Fluent message manifest");
    let messages = source
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (id, raw_arguments) = line.split_once('|').expect("message separator");
            let arguments = raw_arguments
                .split(',')
                .map(|argument| {
                    let (name, type_name) = argument.split_once(':').expect("type separator");
                    let type_ = match type_name {
                        "text" => SchemaType::Text,
                        "number" => SchemaType::Number,
                        "boolean" => SchemaType::Boolean,
                        other => SchemaType::Enum(other.to_string()),
                    };
                    MessageArgument {
                        name: name.to_string(),
                        type_,
                    }
                })
                .collect();
            MessageDefinition {
                id: id.to_string(),
                arguments,
            }
        })
        .collect();
    MessageManifest { messages }
}

fn formatter_error(message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(
        DiagnosticCode::FORMATTER,
        Severity::Error,
        None,
        message,
    ))
}

struct FluentTestFormatter {
    bundles: Vec<(String, Bundle)>,
}

impl FluentTestFormatter {
    fn load(directory: &std::path::Path, locales: &[&str]) -> Self {
        let bundles = locales
            .iter()
            .map(|locale| {
                let language = locale
                    .parse()
                    .expect("fixture locale is a valid language ID");
                let mut bundle = Bundle::new(vec![language]);
                let source = fs::read_to_string(directory.join(format!("{locale}.ftl")))
                    .expect("read Fluent fixture");
                let resource = FluentResource::try_new(source).unwrap_or_else(|(_, errors)| {
                    panic!("invalid {locale} Fluent fixture: {errors:?}")
                });
                bundle.add_resource(resource).unwrap_or_else(|errors| {
                    panic!("conflicting {locale} Fluent fixture: {errors:?}")
                });
                ((*locale).to_string(), bundle)
            })
            .collect();
        Self { bundles }
    }
}

impl Formatter for FluentTestFormatter {
    fn format(&mut self, request: &FormatterRequest) -> MecoResult<FormatterResult> {
        let (actual_locale, bundle) = core::iter::once(request.requested_locale())
            .chain(request.fallback_locales().iter().map(String::as_str))
            .find_map(|candidate| {
                self.bundles
                    .iter()
                    .find(|(locale, _)| locale == candidate)
                    .map(|(locale, bundle)| (locale.clone(), bundle))
            })
            .ok_or_else(|| formatter_error("no requested or fallback Fluent bundle is loaded"))?;

        let mut arguments = FluentArgs::new();
        for (name, value) in request.arguments() {
            match value {
                Value::Text(value) | Value::Enum(value) => arguments.set(name, value.as_str()),
                Value::Number(value) if value.denominator() == 1 => {
                    arguments.set(name, value.numerator());
                }
                Value::Number(_) => {
                    return Err(formatter_error(
                        "the test adapter accepts only integral Fluent numbers",
                    ));
                }
                Value::Boolean(value) => arguments.set(name, value.to_string()),
            }
        }

        let message = bundle
            .get_message(request.message_id())
            .ok_or_else(|| formatter_error("Fluent message is missing"))?;
        let pattern = message
            .value()
            .ok_or_else(|| formatter_error("Fluent message has no value"))?;
        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(pattern, Some(&arguments), &mut errors)
            .into_owned();
        if !errors.is_empty() {
            return Err(formatter_error(format!(
                "Fluent formatting failed: {errors:?}"
            )));
        }

        Ok(FormatterResult {
            text,
            actual_locale: actual_locale.clone(),
            environment_hash: format!("fluent-bundle/0.16.0|fluent-demo/v1|{actual_locale}"),
            diagnostics: vec![],
            work_units: 1,
            replayable: true,
        })
    }
}

fn request_data(name: &str, count: i64, gender: &str) -> Vec<DataBinding> {
    vec![
        DataBinding::new("playerName".to_string(), Value::Text(name.to_string())),
        DataBinding::new(
            "itemCount".to_string(),
            Value::Number(Rational::new(count, 1).expect("valid fixture count")),
        ),
        DataBinding::new("gender".to_string(), Value::Enum(gender.to_string())),
    ]
}

#[test]
fn real_fluent_resources_receive_typed_arguments_and_select_gender_and_plurals() {
    let directory = fixture_directory();
    let root_path = directory.join("root.meco");
    let package = PackageInput {
        root_id: "root".to_string(),
        modules: vec![PackageSource {
            canonical_id: "root".to_string(),
            source: SourceFile::new(
                SourceId::new(0),
                root_path.display().to_string(),
                fs::read_to_string(root_path).expect("read Fluent grammar fixture"),
            ),
            resolved_imports: vec![],
        }],
    };
    let grammar = compile_package_with_manifest(&package, &read_manifest(&directory))
        .expect("Fluent grammar and message schema compile");
    let mut formatter = FluentTestFormatter::load(&directory, &["en-US", "it-IT"]);

    let cases = [
        (
            "en-US",
            "Ada",
            1,
            "female",
            "\u{2068}Ms. \u{2068}Ada\u{2069}\u{2069} arrived with \u{2068}one item\u{2069}.",
        ),
        (
            "en-US",
            "Alex",
            2,
            "male",
            "\u{2068}Mr. \u{2068}Alex\u{2069}\u{2069} arrived with \u{2068}\u{2068}2\u{2069} items\u{2069}.",
        ),
        (
            "it-IT",
            "Ada",
            1,
            "female",
            "\u{2068}La viaggiatrice \u{2068}Ada\u{2069} è arrivata\u{2069} con \u{2068}un oggetto\u{2069}.",
        ),
        (
            "it-IT",
            "Luca",
            2,
            "male",
            "\u{2068}Il viaggiatore \u{2068}Luca\u{2069} è arrivato\u{2069} con \u{2068}\u{2068}2\u{2069} oggetti\u{2069}.",
        ),
        (
            "it-IT",
            "Alex",
            3,
            "other",
            "\u{2068}La persona \u{2068}Alex\u{2069} è arrivata\u{2069} con \u{2068}\u{2068}3\u{2069} oggetti\u{2069}.",
        ),
    ];

    for (locale, name, count, gender, expected) in cases {
        let data = request_data(name, count, gender);
        let generated = grammar
            .generate_weighted_with_formatter(
                &GenerationRequest {
                    data: &data,
                    ..GenerationRequest::with_seed(0)
                },
                LocaleRequest {
                    requested: locale,
                    fallbacks: &[],
                },
                &mut formatter,
            )
            .expect("Fluent-backed generation succeeds");
        assert_eq!(generated.text(), expected, "{locale}/{gender}/{count}");
        let trace = generated.message().expect("message trace is recorded");
        assert_eq!(trace.message_id(), "arrival");
        assert_eq!(trace.requested_locale(), locale);
        assert_eq!(trace.actual_locale(), locale);
        assert!(trace.environment_hash().contains("fluent-bundle/0.16.0"));
        assert!(trace.replayable());
    }

    let data = request_data("Ada", 2, "female");
    let fallback = grammar
        .generate_weighted_with_formatter(
            &GenerationRequest {
                data: &data,
                ..GenerationRequest::with_seed(0)
            },
            LocaleRequest {
                requested: "fr-FR",
                fallbacks: &["it-IT", "en-US"],
            },
            &mut formatter,
        )
        .expect("explicit fallback resolves through Fluent");
    let trace = fallback.message().expect("fallback trace is recorded");
    assert_eq!(trace.requested_locale(), "fr-FR");
    assert_eq!(trace.actual_locale(), "it-IT");
    assert_eq!(
        fallback.text(),
        "\u{2068}La viaggiatrice \u{2068}Ada\u{2069} è arrivata\u{2069} con \u{2068}\u{2068}2\u{2069} oggetti\u{2069}."
    );
}
