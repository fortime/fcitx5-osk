#[cfg(feature = "custom-action-http-api")]
use std::path::PathBuf;

use getset::Getters;
use iced::Font;
use serde::Deserialize;

#[cfg(feature = "custom-action-http-api")]
pub mod http_api {
    use std::{collections::HashMap, result::Result as StdResult, sync::Arc};

    use anyhow::Result;
    use base64::{prelude::BASE64_STANDARD_NO_PAD, Engine as _};
    use iced::futures::channel::mpsc::UnboundedSender;
    use reqwest::{
        header::{HeaderMap, HeaderName},
        Method, Url,
    };
    use serde::{de::Error, Deserialize, Deserializer};

    use crate::{
        app::Message, custom_action::CustomActionCandidate, font, key_set::ComboKeyGroup,
        state::KeyboardEvent,
    };

    struct MethodWrapper(Method);

    impl<'de> Deserialize<'de> for MethodWrapper {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let method: String = Deserialize::deserialize(deserializer)?;
            match method.parse() {
                Ok(m) => Ok(Self(m)),
                Err(_) => Err(Error::custom("Invalid method")),
            }
        }
    }

    struct BodyWrapper(Option<Vec<u8>>);

    impl<'de> Deserialize<'de> for BodyWrapper {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let body: Option<&str> = Deserialize::deserialize(deserializer)?;
            if let Some(body) = body {
                if let Ok(body) = BASE64_STANDARD_NO_PAD.decode(body) {
                    Ok(BodyWrapper(Some(body)))
                } else {
                    Err(Error::custom("Invalid base64 encoding of body"))
                }
            } else {
                Ok(BodyWrapper(None))
            }
        }
    }

    #[derive(Deserialize)]
    struct Target {
        headers_name: Option<String>,
        body_name: Option<String>,
        query_name: Option<String>,
        url: String,
        method: MethodWrapper,
    }

    #[derive(Deserialize)]
    struct HttpApiParamsInner {
        #[serde(default)]
        headers: HashMap<String, HashMap<String, Vec<String>>>,
        #[serde(default)]
        queries: HashMap<String, HashMap<String, String>>,
        #[serde(default)]
        bodies: HashMap<String, BodyWrapper>,
        targets: Vec<Target>,
    }

    #[derive(Clone)]
    pub struct HttpApiParams {
        inner: Arc<HttpApiParamsInner>,
    }

    impl<'de> Deserialize<'de> for HttpApiParams {
        fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let inner: HttpApiParamsInner = Deserialize::deserialize(deserializer)?;
            Ok(Self {
                inner: Arc::new(inner),
            })
        }
    }

    #[derive(Deserialize)]
    pub struct HttpApiResponse {
        prompts: Vec<Vec<(String, Option<String>)>>,
        groups: Vec<ComboKeyGroup>,
        next: Option<String>,
    }

    pub async fn execute(
        tx: UnboundedSender<Message>,
        serial: u32,
        params: HttpApiParams,
        should_stop: impl Fn(u32) -> bool,
    ) -> Result<()> {
        let client = reqwest::Client::new();
        let params = params.inner;
        let default_headers = Default::default();
        let default_query = Default::default();
        for target in &params.targets {
            let mut headers = HeaderMap::new();
            for (h, l) in target
                .headers_name
                .as_ref()
                .and_then(|n| params.headers.get(n))
                .unwrap_or(&default_headers)
            {
                for v in l {
                    headers.append(h.parse::<HeaderName>()?, v.parse()?);
                }
            }
            let query = target
                .query_name
                .as_ref()
                .and_then(|n| params.queries.get(n))
                .unwrap_or(&default_query);
            let mut body = target
                .body_name
                .as_ref()
                .and_then(|n| params.bodies.get(n).and_then(|b| b.0.as_ref()));

            let mut url: Url = target.url.parse()?;
            url.query_pairs_mut().extend_pairs(query.iter());

            loop {
                if should_stop(serial) {
                    tracing::warn!("Serial[{serial}] is changed, skip url[{url}]");
                    break;
                }
                let mut req_builder = client.request(target.method.0.clone(), url.clone());
                req_builder = req_builder.headers(headers.clone());
                // body is used once
                if let Some(body) = body.take() {
                    req_builder = req_builder.body(body.clone());
                }

                let resp = req_builder.send().await?;
                if !resp.status().is_success() {
                    tracing::debug!(
                        "Error response, url: {url}, status: {}, message: {}",
                        resp.status(),
                        resp.text()
                            .await
                            .unwrap_or_else(|_| "Unknown error".to_string())
                    );
                    anyhow::bail!("Calling url[{url}] error");
                }

                let resp: HttpApiResponse = resp.json().await?;

                let mut candidates = vec![];
                for prompt in resp.prompts {
                    let text = prompt
                        .into_iter()
                        .map(|(t, f)| (t, f.map(|f| font::load(&f))))
                        .collect();
                    candidates.push(CustomActionCandidate::Prompt(text));
                }
                for group in resp.groups {
                    candidates.push(CustomActionCandidate::Keys(group.keys));
                }
                tx.unbounded_send(
                    KeyboardEvent::ExtendCustomActionCandidate((serial, candidates)).into(),
                )?;

                if let Some(next) = resp.next {
                    url = next.parse()?;
                } else {
                    break;
                }
            }
            //reqwest::get()
        }
        todo!()
    }
}

use crate::{
    key_set::{ComboKey, ComboKeyGroup},
    store::IdAndConfigPath,
};

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum CustomActionKind {
    Static {
        groups: Vec<ComboKeyGroup>,
    },
    #[cfg(feature = "custom-action-http-api")]
    HttpApi(http_api::HttpApiParams),
}

#[derive(Deserialize, Getters)]
pub(crate) struct CustomAction {
    path: Option<PathBuf>,
    #[getset(get = "pub")]
    name: String,
    #[getset(get = "pub")]
    action: CustomActionKind,
}

impl IdAndConfigPath for CustomAction {
    type IdType = String;

    fn id(&self) -> &Self::IdType {
        &self.name
    }

    fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    fn set_path<T: Into<PathBuf>>(&mut self, path: T) {
        self.path = Some(path.into());
    }
}

#[derive(Clone, Debug)]
pub enum CustomActionCandidate {
    #[allow(unused)]
    Prompt(Vec<(String, Option<Font>)>),
    Keys(Vec<ComboKey>),
}
