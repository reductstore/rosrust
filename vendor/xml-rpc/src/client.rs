use super::error::{Result, ResultExt};
use super::xmlfmt::{from_params, into_params, parse, Call, Fault, Params, Response};
use reqwest::blocking::Client as HttpClient;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use std;
use Url;

pub fn call_value<Tkey>(uri: &Url, name: Tkey, params: Params) -> Result<Response>
where
    Tkey: Into<String>,
{
    Client::new()?.call_value(uri, name, params)
}

pub fn call<'a, Tkey, Treq, Tres>(
    uri: &Url,
    name: Tkey,
    req: Treq,
) -> Result<std::result::Result<Tres, Fault>>
where
    Tkey: Into<String>,
    Treq: Serialize,
    Tres: Deserialize<'a>,
{
    Client::new()?.call(uri, name, req)
}

pub struct Client {
    client: HttpClient,
}

impl Client {
    pub fn new() -> Result<Client> {
        let client = HttpClient::builder()
            .build()
            .chain_err(|| "Failed to initialize HTTP client.")?;
        Ok(Client { client })
    }

    pub fn call_value<Tkey>(&mut self, uri: &Url, name: Tkey, params: Params) -> Result<Response>
    where
        Tkey: Into<String>,
    {
        use super::xmlfmt::value::ToXml;
        let body = Call {
            name: name.into(),
            params,
        }
        .to_xml();

        let response = self
            .client
            .post(uri.clone())
            .header(CONTENT_TYPE, "text/xml")
            .body(body)
            .send()
            .chain_err(|| "Failed to run the HTTP request.")?;

        parse::response(response).map_err(Into::into)
    }

    pub fn call<'a, Tkey, Treq, Tres>(
        &mut self,
        uri: &Url,
        name: Tkey,
        req: Treq,
    ) -> Result<std::result::Result<Tres, Fault>>
    where
        Tkey: Into<String>,
        Treq: Serialize,
        Tres: Deserialize<'a>,
    {
        match self.call_value(uri, name, into_params(&req)?) {
            Ok(Ok(v)) => from_params(v).map(Ok).map_err(Into::into),
            Ok(Err(v)) => Ok(Err(v)),
            Err(v) => Err(v),
        }
    }
}
