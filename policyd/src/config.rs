/*
 * policyd/src/config.rs - TOML config parser for policy daemon
 *
 * Parses a minimal TOML-like config file with the following format:
 *
 *   [policy]
 *   path = "/etc/serix/policy.toml"
 *
 *   [[rule]]
 *   target = "/path/to/resource"
 *   match_type = "exact" | "prefix" | "glob"
 *   action = "allow" | "deny"
 *   required_rights = "READ", "WRITE", "READ,WRITE"
 *
 * This is a custom parser (not the `toml` crate) to keep policyd
 * self-contained in no_std without pulling in serde.
 */

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use fs::policy::{MatchType, PolicyAction, PolicyRule};

/* ------------------------------------------------------------------ */
/*  Config model                                                       */
/* ------------------------------------------------------------------ */

/// A parsed config entry: a single rule with its settings.
pub struct ConfigRule {
	pub path: String,
	pub match_type: MatchType,
	pub action: PolicyAction,
	pub required_rights: u32, // bitmask, converted to capability::Rights later
}

/// Parse a TOML-like config string into a list of rules.
///
/// Supports:
///   - Section headers: [section], [section.subsection], [[array]]
///   - Key-value pairs: key = value
///   - String values: "quoted" or bare (unquoted)
///   - Arrays: ["a", "b", "c"]
///   - Comments: # until end of line
///   - Whitespace (leading/trailing) is trimmed on all values.
pub fn parse_config(text: &str) -> Vec<ConfigRule> {
	let mut rules = Vec::new();
	let mut current_rule: Option<ConfigRule> = None;

	for raw_line in text.lines() {
		let line = trim_comments(raw_line).trim();

		if line.is_empty() || (line.starts_with('[') && !line.starts_with("[[")) {
			/* Section header — flush any pending rule */
			if let Some(rule) = current_rule.take() {
				rules.push(rule);
			}
			continue;
		}

		if line == "[[rule]]" {
			/* New rule block — flush previous */
			if let Some(rule) = current_rule.take() {
				rules.push(rule);
			}
			current_rule = Some(ConfigRule {
				path: String::new(),
				match_type: MatchType::Exact,
				action: PolicyAction::Allow,
				required_rights: 0,
			});
			continue;
		}

		/* Key-value pair */
		if let Some((key, value)) = split_kv(line) {
			let key = trim(key);
			let value = trim(value);

			let rule = current_rule.get_or_insert_with(|| ConfigRule {
				path: String::new(),
				match_type: MatchType::Exact,
				action: PolicyAction::Allow,
				required_rights: 0,
			});

			match key {
				"target" => rule.path = parse_string(value),
				"match_type" => rule.match_type = parse_match_type(value),
				"action" => rule.action = parse_action(value),
				"required_rights" => rule.required_rights = parse_rights(value),
				_ => {} /* unknown key — ignore */
			}
		}
	}

	/* Flush last rule */
	if let Some(rule) = current_rule.take() {
		rules.push(rule);
	}

	rules
}

/* ------------------------------------------------------------------ */
/*  Parsing helpers                                                    */
/* ------------------------------------------------------------------ */

/// Strip comments (# ... to end of line), respecting quoted strings.
fn trim_comments(line: &str) -> &str {
	let mut in_string = false;
	let mut escape = false;

	for (i, ch) in line.char_indices() {
		if escape {
			escape = false;
			continue;
		}
		if ch == '\\' && in_string {
			escape = true;
			continue;
		}
		if ch == '"' {
			in_string = !in_string;
			continue;
		}
		if ch == '#' && !in_string {
			return &line[..i];
		}
	}
	line
}

fn trim(s: &str) -> &str {
	s.trim()
}

/// Split "key = value" into (key, value).
fn split_kv(line: &str) -> Option<(&str, &str)> {
	let eq_pos = line.find('=')?;
	let key = &line[..eq_pos];
	let value = &line[eq_pos + 1..];
	Some((key, value))
}

/// Parse a string value: remove surrounding quotes if present.
fn parse_string(value: &str) -> String {
	let value = value.trim();
	if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
		value[1..value.len() - 1].to_string()
	} else {
		value.to_string()
	}
}

/// Parse a match_type value: "exact", "prefix", "glob".
fn parse_match_type(value: &str) -> MatchType {
	match value.trim().to_lowercase().as_str() {
		"exact" => MatchType::Exact,
		"prefix" => MatchType::Prefix,
		"glob" => MatchType::Glob,
		_ => MatchType::Exact,
	}
}

/// Parse an action value: "allow" or "deny".
fn parse_action(value: &str) -> PolicyAction {
	match value.trim().to_lowercase().as_str() {
		"allow" => PolicyAction::Allow,
		"deny" => PolicyAction::Deny,
		_ => PolicyAction::Allow,
	}
}

/// Parse a rights value: "READ", "WRITE", "READ,WRITE", etc.
/// Returns a bitmask (matching capability::Rights bit positions).
fn parse_rights(value: &str) -> u32 {
	let mut bits: u32 = 0;
	for token in value.split(',') {
		let token = trim(token).to_lowercase();
		bits |= match token.as_str() {
			"read" => 1 << 0,
			"write" => 1 << 1,
			"execute" => 1 << 2,
			"seek" => 1 << 3,
			"append" => 1 << 4,
			"metadata" => 1 << 5,
			"lookup" => 1 << 6,
			"send" => 1 << 7,
			"recv" => 1 << 8,
			"notify" => 1 << 9,
			"control" => 1 << 10,
			"mmap" => 1 << 11,
			"munmap" => 1 << 12,
			"spawn" => 1 << 13,
			"wait" => 1 << 14,
			"dup" => 1 << 15,
			"platform_admin" => 1 << 16,
			_ => 0,
		};
	}
	bits
}

/* ------------------------------------------------------------------ */
/*  ConfigRule -> PolicyRule conversion                               */
/* ------------------------------------------------------------------ */

/// Convert parsed config rules to policy engine rules.
pub fn config_rules_to_policy(config_rules: &[ConfigRule]) -> Vec<PolicyRule> {
	config_rules
		.iter()
		.map(|cr| PolicyRule {
			path: cr.path.clone(),
			match_type: cr.match_type,
			required_rights: capability::Rights(cr.required_rights),
			action: cr.action,
		})
		.collect()
}

/* ------------------------------------------------------------------ */
/*  Unit tests                                                         */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_empty_config() {
		let rules = parse_config("");
		assert!(rules.is_empty());
	}

	#[test]
	fn parse_single_rule() {
		let text = "\
[[rule]]\
target = \"/etc/shadow\"\
match_type = \"exact\"\
action = \"deny\"\
required_rights = \"READ\"\
";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 1);
		assert_eq!(rules[0].path, "/etc/shadow");
		assert_eq!(rules[0].match_type, MatchType::Exact);
		assert_eq!(rules[0].action, PolicyAction::Deny);
		assert_eq!(rules[0].required_rights, 1); /* READ = 1 << 0 */
	}

	#[test]
	fn parse_multiple_rules() {
		let text = "\
[[rule]]\
target = \"/etc/shadow\"\
match_type = \"exact\"\
action = \"deny\"\
required_rights = \"READ\"\

[[rule]]\
target = \"/home\"\
match_type = \"prefix\"\
action = \"allow\"\
required_rights = \"READ,WRITE\"\
";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 2);
		assert_eq!(rules[0].path, "/etc/shadow");
		assert_eq!(rules[1].path, "/home");
		assert_eq!(rules[1].match_type, MatchType::Prefix);
		assert_eq!(rules[1].action, PolicyAction::Allow);
		assert_eq!(rules[1].required_rights, (1 << 0) | (1 << 1)); /* READ | WRITE */
	}

	#[test]
	fn parse_glob_match() {
		let text = "\
[[rule]]\
target = \"/tmp/*.log\"\
match_type = \"glob\"\
action = \"allow\"\
required_rights = \"READ\"\
";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 1);
		assert_eq!(rules[0].match_type, MatchType::Glob);
	}

	#[test]
	fn parse_bare_string_values() {
		let text = "\
[[rule]]\
target = /etc/passwd\
match_type = exact\
action = allow\
";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 1);
		assert_eq!(rules[0].path, "/etc/passwd");
		assert_eq!(rules[0].match_type, MatchType::Exact);
		assert_eq!(rules[0].action, PolicyAction::Allow);
	}

	#[test]
	fn parse_comments() {
		let text = "\
# This is a full-line comment\
[[rule]]\
target = \"/etc/shadow\" # inline comment\
action = \"deny\" # deny all\
";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 1);
		assert_eq!(rules[0].path, "/etc/shadow");
	}

	#[test]
	fn parse_rights_bitmask() {
		assert_eq!(parse_rights("READ"), 1 << 0);
		assert_eq!(parse_rights("WRITE"), 1 << 1);
		assert_eq!(parse_rights("READ,WRITE"), (1 << 0) | (1 << 1));
		assert_eq!(parse_rights("EXECUTE,SEEK"), (1 << 2) | (1 << 3));
	}

	#[test]
	fn parse_section_headers_flush_rule() {
		let text = "\
[[rule]]\
target = \"/first\"\
action = \"allow\"\

[section.other]\

[[rule]]\
target = \"/second\"\
action = \"deny\"\
";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 1); /* Only the second rule (first was flushed) */
		assert_eq!(rules[0].path, "/second");
	}

	#[test]
	fn parse_unknown_keys_ignored() {
		let text = "\
[[rule]]\
target = \"/test\"\
unknown_key = \"ignored\"\
action = \"allow\"\
";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 1);
		assert_eq!(rules[0].path, "/test");
	}

	#[test]
	fn parse_empty_lines_ignored() {
		let text = "\

[[rule]]\

target = \"/test\"\

action = \"deny\"\

";
		let rules = parse_config(text);
		assert_eq!(rules.len(), 1);
		assert_eq!(rules[0].path, "/test");
	}

	#[test]
	fn config_rules_to_policy_conversion() {
		let config_rules = vec![ConfigRule {
			path: "/test".to_string(),
			match_type: MatchType::Exact,
			action: PolicyAction::Deny,
			required_rights: 0,
		}];
		let policy_rules = config_rules_to_policy(&config_rules);
		assert_eq!(policy_rules.len(), 1);
		assert_eq!(policy_rules[0].path, "/test");
		assert_eq!(policy_rules[0].action, PolicyAction::Deny);
	}
}
