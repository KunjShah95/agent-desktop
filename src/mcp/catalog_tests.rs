use super::*;
use clap::Parser;
use clap::error::ErrorKind;

use crate::cli::Commands;

fn sample(schema: &Value) -> Value {
    let item = if schema["type"] == "array" {
        &schema["items"]
    } else {
        schema
    };
    let value = match (item["type"].as_str(), item["enum"].get(0)) {
        (_, Some(first)) => first.clone(),
        (Some("boolean"), _) => Value::Bool(true),
        (Some("integer" | "number"), _) => Value::from(1),
        _ => Value::from("x"),
    };
    if schema["type"] == "array" {
        Value::Array(vec![value])
    } else {
        value
    }
}

fn parse(tool: &Tool, arguments: &Map<String, Value>) -> Result<Cli, clap::Error> {
    let argv = tool
        .argv(arguments)
        .expect("schema-shaped arguments build argv");
    Cli::try_parse_from(std::iter::once("agent-desktop".to_string()).chain(argv))
}

#[test]
fn every_schema_property_is_accepted_by_the_cli_parser() {
    let mut checked = 0;
    for tool in tools(false) {
        let definition = tool.definition();
        let schema = &definition["inputSchema"];
        let properties = schema["properties"].as_object().expect("properties");
        let required = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|key| {
                let key = key.as_str().expect("required property name");
                (key.to_owned(), sample(&properties[key]))
            })
            .collect::<Map<_, _>>();
        for (key, schema) in properties {
            let mut arguments = required.clone();
            arguments.insert(key.clone(), sample(schema));
            let Err(error) = parse(&tool, &arguments) else {
                checked += 1;
                continue;
            };
            let plain_string = schema["type"] == "string" && schema.get("enum").is_none();
            let tolerated = matches!(
                error.kind(),
                ErrorKind::MissingRequiredArgument | ErrorKind::ArgumentConflict
            ) || (plain_string && error.kind() == ErrorKind::ValueValidation);
            assert!(
                tolerated,
                "{} rejected its own schema property '{key}': {error}",
                tool.name
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} properties were checked");
}

#[test]
fn every_tool_runs_the_command_it_is_named_after() {
    for tool in tools(false) {
        let definition = tool.definition();
        if let Some(name) = definition["inputSchema"]["required"][0].as_str() {
            assert_eq!(
                tool.argv(&Map::new()).unwrap_err(),
                format!("Missing required argument '{name}'"),
                "{}",
                tool.name
            );
            continue;
        }
        let error = match parse(&tool, &Map::new()) {
            Ok(cli) => {
                let command = cli.command.expect("a command");
                assert_eq!(command.name(), tool.path[0], "{}", tool.name);
                continue;
            }
            Err(error) => error,
        };
        assert!(
            error.kind() == ErrorKind::MissingRequiredArgument,
            "{} failed to parse with no arguments: {error}",
            tool.name
        );
    }
}

#[test]
fn tool_names_are_unique_and_valid_for_every_major_client() {
    let names = tools(false)
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    let unique = names.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), names.len(), "duplicate tool names: {names:?}");
    for name in &names {
        assert!(
            name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{name} is not a portable tool name"
        );
    }
}

#[test]
fn unknown_arguments_and_wrong_types_are_reported_not_passed_through() {
    let tool = tools(false)
        .into_iter()
        .find(|tool| tool.path == ["skills", "get"])
        .expect("skills get tool");
    let mut arguments = Map::new();
    arguments.insert("name".into(), Value::from("desktop"));
    arguments.insert("full".into(), Value::from(true));
    arguments.insert("reference".into(), Value::Null);
    assert_eq!(
        tool.argv(&arguments).unwrap(),
        ["skills", "get", "--full", "--", "desktop"]
    );
    arguments.insert("full".into(), Value::from("yes"));
    assert!(tool.argv(&arguments).unwrap_err().contains("boolean"));
    arguments.remove("full");
    arguments.insert("headed".into(), Value::from(true));
    assert!(
        tool.argv(&arguments)
            .unwrap_err()
            .contains("Unknown argument 'headed'")
    );
}

fn tool_at(path: &[&str]) -> Tool {
    tools(false)
        .into_iter()
        .find(|tool| tool.path == path)
        .expect("tool exists")
}

#[test]
fn missing_required_arguments_are_rejected_before_positionals_can_shift() {
    let tool = tool_at(&["skills", "get"]);
    let mut arguments = Map::new();
    arguments.insert("reference".into(), Value::from("desktop"));

    assert_eq!(
        tool.argv(&arguments).unwrap_err(),
        "Missing required argument 'name'"
    );

    arguments.insert("name".into(), Value::Null);
    assert_eq!(
        tool.argv(&arguments).unwrap_err(),
        "Missing required argument 'name'"
    );
}

#[test]
fn arrays_and_booleans_are_rejected_for_string_arguments() {
    let tool = tool_at(&["skills", "get"]);
    for wrong in [Value::from(false), Value::Array(vec![Value::from("a")])] {
        let mut arguments = Map::new();
        arguments.insert("name".into(), wrong);
        assert_eq!(
            tool.argv(&arguments).unwrap_err(),
            "Argument 'name' must be a JSON string"
        );
    }
}

#[test]
fn inline_text_commands_require_their_text_field() {
    for (path, field) in [(&["type"][..], "text"), (&["set-value"][..], "value")] {
        let tool = tool_at(path);
        let required = tool.definition()["inputSchema"]["required"]
            .as_array()
            .expect("required")
            .clone();
        assert!(
            required.iter().any(|key| key.as_str() == Some(field)),
            "{} must advertise '{field}' as required: {required:?}",
            tool.name
        );

        let mut arguments = Map::new();
        arguments.insert("ref_id".into(), Value::from("@s1:e1"));
        assert_eq!(
            tool.argv(&arguments).unwrap_err(),
            format!("Missing required argument '{field}'")
        );
    }
}

#[test]
fn launch_options_preserve_array_order_and_reject_wrong_types() {
    let tool = tool_at(&["launch"]);
    let input = serde_json::json!({
        "app": "example",
        "args": ["--headed", "argument with spaces", ""],
        "timeout": 2500
    });
    let arguments = input.as_object().unwrap();
    let cli = parse(&tool, arguments).unwrap();
    assert!(!cli.interaction.headed);
    let Some(Commands::Launch(launch)) = cli.command else {
        panic!("launch");
    };
    assert_eq!(launch.args, ["--headed", "argument with spaces", ""]);
    assert_eq!(launch.timeout, 2500);

    for (key, value, expected) in [
        (
            "args",
            serde_json::json!("--headed"),
            "an array of string values",
        ),
        (
            "args",
            serde_json::json!(["--headed", false]),
            "an array of string values",
        ),
        ("timeout", serde_json::json!("2500"), "a JSON integer"),
        ("timeout", serde_json::json!(2.5), "a JSON integer"),
    ] {
        let mut wrong = arguments.clone();
        wrong.insert(key.into(), value);
        assert_eq!(
            tool.argv(&wrong).unwrap_err(),
            format!("Argument '{key}' must be {expected}")
        );
    }
}

#[test]
fn a_read_only_server_exposes_only_observation_and_metadata_tools() {
    let read_only = tools(true);
    let names = read_only
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();

    assert!(!read_only.is_empty(), "a read-only server exposes tools");
    for required in ["desktop_snapshot", "desktop_get", "desktop_list_windows"] {
        assert!(names.contains(&required), "missing {required}: {names:?}");
    }
    for forbidden in [
        "desktop_click",
        "desktop_type",
        "desktop_press",
        "desktop_clipboard_set",
        "desktop_launch",
        "desktop_close_app",
        "desktop_screenshot",
        "desktop_clipboard_get",
        "desktop_session_start",
        "desktop_trace_export",
        "desktop_batch",
    ] {
        assert!(
            !names.contains(&forbidden),
            "a read-only server must not expose {forbidden}"
        );
    }
    for tool in read_only {
        assert!(
            READ_ONLY_TOOL_NAMES.contains(&tool.name.as_str()),
            "{} is not in the reviewed read-only allow-list",
            tool.name
        );
    }
    assert!(
        tools(false).iter().any(|tool| tool.name == "desktop_click"),
        "the default server still exposes mutating tools"
    );
}

#[test]
fn flag_shaped_values_stay_values_and_never_become_flags() {
    let hostile = "--headed --trace=/tmp/owned -- --session=x";
    let mut arguments = Map::new();
    arguments.insert("app".into(), Value::from(hostile));
    let cli = parse(&tool_at(&["snapshot"]), &arguments).expect("parses");
    assert!(!cli.interaction.headed && cli.trace.is_none() && cli.identity.session.is_none());
    let Some(crate::cli::Commands::Snapshot(snapshot)) = cli.command else {
        panic!("snapshot");
    };
    assert_eq!(snapshot.scope.app.as_deref(), Some(hostile));

    let mut arguments = Map::new();
    arguments.insert("ref_id".into(), Value::from("@s8f3k2p9:e1"));
    arguments.insert("text".into(), Value::from(hostile));
    let cli = parse(&tool_at(&["type"]), &arguments).expect("parses");
    assert!(!cli.interaction.headed && cli.trace.is_none() && cli.identity.session.is_none());
    let Some(crate::cli::Commands::Type(typed)) = cli.command else {
        panic!("type");
    };
    assert_eq!(typed.text.as_deref(), Some(hostile));
}

#[test]
fn global_flags_cannot_be_set_per_call() {
    let tool = tool_at(&["snapshot"]);
    for global in [
        "headed",
        "session",
        "agent_id",
        "trace",
        "trace_strict",
        "wait_for",
        "debug",
    ] {
        let mut arguments = Map::new();
        arguments.insert(global.into(), Value::from("x"));
        let error = tool.argv(&arguments).expect_err(global);
        assert!(error.contains("Unknown argument"), "{global}: {error}");
    }
}

#[test]
fn stdin_input_is_not_exposed_to_mcp_calls() {
    for path in [["type"], ["set-value"], ["clipboard-set"]] {
        let tool = tool_at(&path);
        assert!(
            tool.definition()["inputSchema"]["properties"]
                .get("stdin")
                .is_none()
        );
        let arguments = Map::from_iter([("stdin".into(), Value::Bool(true))]);
        assert!(
            tool.argv(&arguments)
                .unwrap_err()
                .contains("Unknown argument 'stdin'")
        );
    }
}
