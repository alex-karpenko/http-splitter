use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer,
};
use std::{
    fmt::Display,
    net::{Ipv4Addr, SocketAddr},
    str::FromStr,
    time::Duration,
};

use crate::{config::target::TargetConditionConfig, errors::HttpDragonflyError};

use super::{
    headers::HeaderTransform,
    response::{ResponseConfig, ResponseStrategy},
    target::TargetConfig,
    ConfigValidator,
};

const DEFAULT_LISTENER_PORT: u16 = 8080;
pub const DEFAULT_LISTENER_TIMEOUT_SEC: u64 = 10;
const INVALID_IP_ADDRESS_ERROR: &str = "IP address isn't valid";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ListenerConfig {
    name: Option<String>,
    #[serde(rename = "on", default)]
    listen_on: ListenOn,
    #[serde(
        with = "humantime_serde",
        default = "ListenerConfig::default_listener_timeout"
    )]
    timeout: Duration,
    headers: Option<Vec<HeaderTransform>>,
    methods: Option<Vec<String>>,
    targets: Vec<TargetConfig>,
    #[serde(default)]
    response: ResponseConfig,
}

impl ListenerConfig {
    fn default_listener_timeout() -> Duration {
        Duration::from_secs(DEFAULT_LISTENER_TIMEOUT_SEC)
    }

    /// Returns the name of this [`ListenerConfig`].
    pub fn name(&self) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else {
            format!("LISTENER-{}", self.listen_on)
        }
    }

    /// Returns the socket of this [`ListenerConfig`].
    pub fn socket(&self) -> SocketAddr {
        self.listen_on.as_socket()
    }

    /// Verifies if HTTP method is allowed to be used call for this [`ListenerConfig`]
    pub fn is_method_allowed(&self, method: &str) -> bool {
        if let Some(methods) = &self.methods {
            let method = method.to_lowercase();
            methods.contains(&method)
        } else {
            true
        }
    }

    /// Returns a reference to the timeout of this [`ListenerConfig`].
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Returns the headers of this [`ListenerConfig`].
    pub fn headers(&self) -> Option<&Vec<HeaderTransform>> {
        self.headers.as_ref()
    }

    /// Returns a reference to the targets of this [`ListenerConfig`].
    pub fn targets(&self) -> &[TargetConfig] {
        self.targets.as_ref()
    }

    /// Returns a reference to the response of this [`ListenerConfig`].
    pub fn response(&self) -> &ResponseConfig {
        &self.response
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

#[derive(Debug)]
struct ListenOn {
    ip: Ipv4Addr,
    port: u16,
}

impl ListenOn {
    fn as_socket(&self) -> SocketAddr {
        SocketAddr::new(self.ip.into(), self.port)
    }
}

impl Default for ListenOn {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::new(0, 0, 0, 0),
            port: DEFAULT_LISTENER_PORT,
        }
    }
}

impl ListenOn {
    fn parse_ip_address(ip: &str) -> Result<Ipv4Addr, String> {
        Ipv4Addr::from_str(ip).map_err(|_| String::from(INVALID_IP_ADDRESS_ERROR))
    }

    fn from_str(v: &str) -> Result<Self, String> {
        let splitted: Vec<_> = v.trim().split(':').collect();

        if splitted.len() == 1 {
            let port: u16 = splitted[0]
                .parse()
                .map_err(|e| format!("invalid port value `{}`: {e}", splitted[0]))?;
            let ip = Ipv4Addr::new(0, 0, 0, 0);

            Ok(ListenOn { ip, port })
        } else if splitted.len() == 2 {
            let port: u16 = splitted[1]
                .parse()
                .map_err(|e| format!("invalid port value `{}`: {e}", splitted[1]))?;

            let ip = if splitted[0].is_empty() || splitted[0] == "*" {
                Ipv4Addr::new(0, 0, 0, 0)
            } else {
                Self::parse_ip_address(splitted[0])?
            };

            Ok(ListenOn { ip, port })
        } else {
            Err("invalid `listen on` token, should be in form IP:PORT".into())
        }
    }
}

impl Display for ListenOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.ip, self.port)
    }
}

impl<'de> Deserialize<'de> for ListenOn {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ListenOnVisitor;
        impl<'de> Visitor<'de> for ListenOnVisitor {
            type Value = ListenOn;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an IP address (or `0.0.0.0` or `*`) and port separated by colon, like `1.2.3.4:8080`")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                ListenOn::from_str(v).map_err(|e| E::custom(e))
            }
        }

        deserializer.deserialize_string(ListenOnVisitor)
    }
}

impl ConfigValidator for ListenerConfig {
    fn validate(&self) -> Result<(), crate::errors::HttpDragonflyError> {
        // Make sure all target IDs are unique
        let unique_targets_count = self.targets().iter().map(TargetConfig::id).count();
        if unique_targets_count != self.targets().len() {
            return Err(HttpDragonflyError::InvalidConfig {
                cause: format!(
                    "all target IDs of the listener `{}` should be unique",
                    self.name()
                ),
            });
        }

        // Validate all targets
        for target in self.targets() {
            target.validate()?;
        }

        // Validate response
        self.response().validate()?;

        // Validate strategy requirements
        match self.response().strategy() {
            ResponseStrategy::ConditionalRouting => {
                // Make sure that all targets have condition defined if strategy is conditional_routing
                if self.targets().iter().any(|t| t.condition().is_none()) {
                    return Err(HttpDragonflyError::InvalidConfig {
                        cause: format!("all targets of the listener `{}` must have condition defined because strategy is `{}`", self.name(), self.response().strategy()),
                    });
                }
                // Ensure singe default condition is present
                let default_count = self
                    .targets()
                    .iter()
                    .filter(|t| {
                        matches!(
                            t.condition().as_ref().unwrap(),
                            TargetConditionConfig::Default
                        )
                    })
                    .count();
                if default_count > 1 {
                    return Err(HttpDragonflyError::InvalidConfig {
                        cause: format!(
                            "more than one default target is defined of the listener `{}` but only one is allowed",
                            self.name()
                        ),
                    });
                }
            }
            ResponseStrategy::AlwaysTargetId
            | ResponseStrategy::FailedThenTargetId
            | ResponseStrategy::OkThenTargetId => {
                // Make sure that target_selector has valid target_id specified if strategy is *_target_id
                let target_ids: Vec<String> = self.targets().iter().map(TargetConfig::id).collect();
                if let Some(target_id) = &self.response().target_selector() {
                    if !target_ids.contains(target_id) {
                        return Err(HttpDragonflyError::InvalidConfig {
                            cause: format!("`target_selector` points to unknown target_id `{}` in the listener `{}`", target_id, self.name()),
                        });
                    }
                } else {
                    return Err(HttpDragonflyError::InvalidConfig {
                        cause: format!("`target_selector` should be specified for strategy `{}` in the listener `{}`", self.response().strategy(), self.name()),
                    });
                }
            }
            _ => {}
        };

        Ok(())
    }
}
