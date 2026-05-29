use std::ops::Range;
use std::path::PathBuf;

use getset::Getters;
use iced::Font;
use serde::Deserialize;

#[cfg(feature = "custom-action-http-api")]
pub mod http_api {
    use std::{
        collections::HashMap, mem, ops::Deref, result::Result as StdResult, sync::Arc,
        time::Duration,
    };

    use anyhow::Result;
    use chrono::{DateTime, Utc};
    use iced::futures::channel::mpsc::UnboundedSender;
    use openssl::pkey::{PKey, Private};
    use reqwest::{
        Method, StatusCode, Url,
        header::{HeaderMap, HeaderName, RETRY_AFTER},
    };
    use serde::{
        Deserialize, Deserializer,
        de::{Error, Unexpected},
    };
    use tokio::time;

    use crate::{
        app::Message,
        custom_action::CustomActionCandidate,
        font,
        key_set::{ComboKey, ComboKeyGroup, KeyValue},
        misc::secret_envelope,
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

    #[derive(Deserialize)]
    struct Target {
        headers_name: Option<String>,
        body_name: Option<String>,
        query_name: Option<String>,
        url: String,
        method: MethodWrapper,
        need_encrypt: Option<bool>,
        need_enter: Option<bool>,
        need_mask: Option<bool>,
    }

    #[derive(Deserialize)]
    struct HttpApiParamsInner {
        #[serde(default)]
        headers: HashMap<String, HashMap<String, Vec<String>>>,
        #[serde(default)]
        queries: HashMap<String, HashMap<String, String>>,
        #[serde(default)]
        bodies: HashMap<String, HashMap<String, String>>,
        #[serde(deserialize_with = "deserialize_targets")]
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
        secret: Option<String>,
        next: Option<String>,
    }

    struct ZeroizingString(String);

    impl Drop for ZeroizingString {
        fn drop(&mut self) {
            let mut s = mem::take(&mut self.0);
            unsafe {
                // Safety, s will be dropped immediately
                s.as_bytes_mut().fill(0);
            }
        }
    }

    impl Deref for ZeroizingString {
        type Target = str;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
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
            let body = target.body_name.as_ref().and_then(|n| params.bodies.get(n));

            let mut url: Url = target.url.parse()?;
            url.query_pairs_mut().extend_pairs(query.iter());
            let mut method = target.method.0.clone();

            let key_pair = if target.need_encrypt.unwrap_or_default() {
                let pri_key = secret_envelope::new_private_key()?;
                let pub_key_pem = String::from_utf8(pri_key.public_key_to_pem()?)?;
                Some((pri_key, pub_key_pem))
            } else {
                None
            };
            // Always mask if it needs encrypt
            let need_mask =
                target.need_encrypt.unwrap_or_default() || target.need_mask.unwrap_or_default();
            let need_enter = target.need_enter.unwrap_or_default();

            loop {
                if should_stop(serial) {
                    tracing::warn!("Serial[{serial}] is changed, skip url[{url}]");
                    break;
                }
                let mut req_builder = client.request(method.clone(), url.clone());
                req_builder = req_builder.headers(headers.clone());
                if method == Method::POST || method == Method::PUT {
                    let mut body = body.cloned().unwrap_or_default();
                    if let Some((_, pub_key_pem)) = &key_pair {
                        body.insert("pub_key".to_string(), pub_key_pem.clone());
                    }
                    req_builder = req_builder.json(&body);
                }

                let resp = req_builder.send().await?;
                if resp.status() != StatusCode::OK {
                    if resp.status() == StatusCode::NO_CONTENT {
                        // retry the url when it is 204
                        if let Some(retry_after) = resp
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|r| r.to_str().ok())
                        {
                            let mut retry_after = if let Ok(r) = retry_after.parse::<u64>() {
                                r
                            } else {
                                if let Ok(d) = DateTime::parse_from_rfc2822(retry_after) {
                                    d.signed_duration_since(Utc::now()).num_seconds() as u64;
                                }
                                1
                            };
                            retry_after = retry_after.clamp(1, 15);
                            time::sleep(Duration::from_secs(retry_after)).await;
                        }
                        continue;
                    } else {
                        tracing::debug!(
                            "Error response, url: {url}, status: {}, message: {}",
                            resp.status(),
                            resp.text()
                                .await
                                .unwrap_or_else(|_| "Unknown error".to_string())
                        );
                        anyhow::bail!("Calling url[{url}] error");
                    }
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
                for mut group in resp.groups {
                    let mut keys = mem::take(&mut group.keys);
                    let mask_range = if need_mask { Some(0..keys.len()) } else { None };
                    // Don't mask enter
                    if need_enter && let Some(kv) = KeyValue::from_char('\n') {
                        keys.push(ComboKey::Key(kv));
                    }
                    candidates.push(CustomActionCandidate::Keys { mask_range, keys });
                }
                if let Some(secret) = resp.secret {
                    let text = if let Some((pri_key, _)) = &key_pair {
                        decrypt(pri_key, &secret)?
                    } else {
                        anyhow::bail!("A secret is returned, but `need_encrypt` is false");
                    };
                    let mut keys = Vec::with_capacity(text.len());
                    for c in text.chars() {
                        if let Some(kv) = KeyValue::from_char(c) {
                            keys.push(ComboKey::Key(kv));
                        } else {
                            anyhow::bail!("Can't convert char[{c}] to key event");
                        }
                    }
                    let mask_range = if need_mask { Some(0..keys.len()) } else { None };
                    // Don't mask enter
                    if need_enter && let Some(kv) = KeyValue::from_char('\n') {
                        keys.push(ComboKey::Key(kv));
                    }
                    candidates.push(CustomActionCandidate::Keys { mask_range, keys });
                }
                tx.unbounded_send(
                    KeyboardEvent::PushFrontCustomActionCandidate((serial, candidates)).into(),
                )?;

                if let Some(next) = resp.next {
                    url = next.parse()?;
                    method = Method::GET;
                } else {
                    break;
                }
            }
        }
        Ok(())
    }

    fn decrypt(pri_key: &PKey<Private>, secret: &str) -> Result<ZeroizingString> {
        let data = secret_envelope::decrypt(pri_key, secret)?;
        match String::from_utf8(data) {
            Ok(s) => Ok(ZeroizingString(s)),
            Err(e) => {
                // zeroize
                e.into_bytes().fill(0);
                anyhow::bail!("Secret isn't a valid utf8 string")
            }
        }
    }

    fn deserialize_targets<'de, D>(deserializer: D) -> Result<Vec<Target>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let targets = <HashMap<String, Target>>::deserialize(deserializer)?;
        let mut arr = Vec::with_capacity(targets.len());
        for (idx, target) in targets {
            arr.push((
                idx.parse::<usize>().map_err(|_| {
                    D::Error::invalid_value(Unexpected::Str(&idx), &"a str of usize")
                })?,
                target,
            ));
        }
        arr.sort_unstable_by_key(|i| i.0);
        Ok(arr.into_iter().map(|(_, target)| target).collect())
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
    Prompt(Vec<(String, Option<Font>)>),
    Keys {
        mask_range: Option<Range<usize>>,
        keys: Vec<ComboKey>,
    },
}
