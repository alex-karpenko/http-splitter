pub mod headers;
pub mod listener;
pub mod response;
pub mod target;
use once_cell::sync::OnceCell;
use serde::Deserialize;
use shellexpand::env_with_context_no_errors;
use std::{
    fs::File,
    io::{BufReader, Read},
};
use tracing::{debug, info};

use crate::{context::Context, errors::HttpDragonflyError};

use self::listener::ListenerConfig;

static APP_CONFIG: OnceCell<AppConfig> = OnceCell::new();

pub trait ConfigValidator {
    fn validate(&self) -> Result<(), HttpDragonflyError>;
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    listeners: Vec<ListenerConfig>,
}

impl<'a> AppConfig {
    pub fn new(filename: &String, ctx: &Context) -> Result<&'a AppConfig, HttpDragonflyError> {
        let config = AppConfig::from_file(filename, ctx)?;
        Ok(APP_CONFIG.get_or_init(|| config))
    }

    fn from_file(filename: &String, ctx: &Context) -> Result<AppConfig, HttpDragonflyError> {
        info!("Loading config: {filename}");
        let mut file = File::open(filename)?;
        AppConfig::from_reader(&mut file, ctx)
    }

    fn from_reader(reader: &mut dyn Read, ctx: &Context) -> Result<AppConfig, HttpDragonflyError> {
        let mut reader = BufReader::new(reader);
        let mut buf = String::new();
        reader.read_to_string(&mut buf)?;
        let config = env_with_context_no_errors(&buf, |v| ctx.get(&v.into()));
        let config: AppConfig = serde_yaml::from_str(&config)?;

        debug!("Application config: {:#?}", config);
        match config.validate() {
            Ok(_) => Ok(config),
            Err(e) => Err(e),
        }
    }

    pub fn listeners(&self) -> &[ListenerConfig] {
        self.listeners.as_ref()
    }
}

impl ConfigValidator for AppConfig {
    fn validate(&self) -> Result<(), HttpDragonflyError> {
        if self.listeners().is_empty() {
            return Err(HttpDragonflyError::ValidateConfig {
                cause: String::from("at least one listener must be configured"),
            });
        }

        for listener in self.listeners() {
            listener.validate()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::context::test_context;
    use insta::{assert_debug_snapshot, glob};

    use super::*;

    const TEST_CONFIGS_FOLDER: &str = "../tests/configs";

    #[test]
    fn good_config() {
        let ctx = test_context::get_test_ctx();
        glob!(
            TEST_CONFIGS_FOLDER,
            "good/*.yaml",
            |path| assert_debug_snapshot!(AppConfig::from_file(
                &String::from(path.to_str().unwrap()),
                &ctx
            ))
        );
    }

    #[test]
    fn wrong_config() {
        let ctx = test_context::get_test_ctx();
        glob!(
            TEST_CONFIGS_FOLDER,
            "wrong/*.yaml",
            |path| insta::with_settings!({filters => vec![(
                r#"unable to parse config: listeners\[0\]\.targets\[1\]\.condition: invalid config: found "/" but expected one of "(.+)" at line 9 column 18,"#,
                r#"unable to parse config: listeners[0].targets[1].condition: invalid config: found "/" but expected one of "[LIST OF ALLOWED JQ STATEMENTS]" at line 9 column 18,"#
            )]},
            {assert_debug_snapshot!(AppConfig::from_file(&String::from(path.to_str().unwrap()),&ctx));})
        );
    }
}
