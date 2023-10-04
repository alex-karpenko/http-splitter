use figment::Error;
use regex::Regex;
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer,
};
use std::fmt::Display;

use crate::errors::HttpSplitterError;

use super::headers::HeaderTransform;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct ResponseConfig {
    strategy: ResponseStrategy,
    target_selector: ResponseTargetSelector,
    failure_status_regex: String,
    failure_on_timeout: bool,
    timeout_status: ResponseStatus,
    cancel_unneeded_targets: bool,
    #[serde(rename = "override")]
    override_config: Option<OverrideConfig>,
}

impl Default for ResponseConfig {
    fn default() -> Self {
        Self {
            strategy: Default::default(),
            target_selector: Default::default(),
            failure_status_regex: "4\\d{2}|5\\d{2}".into(),
            failure_on_timeout: true,
            timeout_status: "504 Gateway Timeout".into(),
            cancel_unneeded_targets: false,
            override_config: None,
        }
    }
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum ResponseStrategy {
    AlwaysOverride,
    AlwaysTargetId,
    OkThenFailed,
    OkThenTargetId,
    OkThenOverride,
    FailedThenOk,
    FailedThenTargetId,
    #[default]
    FailedThenOverride,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum ResponseTargetSelector {
    #[default]
    Fastest,
    Slowest,
    Random,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct OverrideConfig {
    status: Option<ResponseStatus>,
    body: Option<String>,
    headers: Option<Vec<HeaderTransform>>,
}

#[derive(Debug)]
struct ResponseStatus {
    code: u16,
    msg: Option<String>,
}

impl<'de> Deserialize<'de> for ResponseStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ResponseStatusVisitor;
        impl<'de> Visitor<'de> for ResponseStatusVisitor {
            type Value = ResponseStatus;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string with three-digit status code and optional status message, i.e. `200 OK`")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                ResponseStatus::from_str(v).map_err(|e| E::custom(e.to_string()))
            }
        }

        deserializer.deserialize_string(ResponseStatusVisitor)
    }
}

impl ResponseStatus {
    fn from_str(v: &str) -> Result<Self, HttpSplitterError> {
        let re = Regex::new(r"^(?P<code>\d{3})\s*(?P<msg>.*)$")
            .unwrap_or_else(|e| panic!("looks like a BUG: {e}"));
        let caps = re.captures(v);

        if let Some(caps) = caps {
            let code: u16 =
                caps["code"]
                    .parse()
                    .map_err(|_e| HttpSplitterError::ParseConfigFile {
                        cause: Error::from(String::from("invalid status string")),
                    })?;
            let msg: Option<String> = match &caps["msg"] {
                "" => None,
                _ => Some(caps["msg"].trim().into()),
            };

            Ok(Self { code, msg })
        } else {
            Err(HttpSplitterError::ParseConfigFile {
                cause: Error::from(String::from("invalid status string")),
            })
        }
    }
}

impl From<&str> for ResponseStatus {
    fn from(value: &str) -> Self {
        ResponseStatus::from_str(value).unwrap()
    }
}

impl Display for ResponseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(msg) = &self.msg {
            write!(f, "{} {}", self.code, msg)
        } else {
            write!(f, "{}", self.code)
        }
    }
}
