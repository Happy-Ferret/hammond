use diesel::SaveChangesDsl;
// use failure::ResultExt;
use rss::Channel;
use url::Url;

use hyper::client::HttpConnector;
use hyper::header::{
    ETag, EntityTag, HttpDate, IfModifiedSince, IfNoneMatch, LastModified, Location, UserAgent,
};
use hyper::{Client, Method, Request, Response, StatusCode, Uri};
use hyper_tls::HttpsConnector;

// use futures::future::ok;
use futures::future::{loop_fn, Future, Loop};
use futures::prelude::*;

use database::connection;
use errors::*;
use feed::{Feed, FeedBuilder};
use models::{NewSource, Save};
use schema::source;
use USER_AGENT;

use std::str::FromStr;

#[derive(Queryable, Identifiable, AsChangeset, PartialEq)]
#[table_name = "source"]
#[changeset_options(treat_none_as_null = "true")]
#[derive(Debug, Clone)]
/// Diesel Model of the source table.
pub struct Source {
    id: i32,
    uri: String,
    last_modified: Option<String>,
    http_etag: Option<String>,
}

impl Save<Source> for Source {
    type Error = DataError;

    /// Helper method to easily save/"sync" current state of self to the
    /// Database.
    fn save(&self) -> Result<Source, Self::Error> {
        let db = connection();
        let con = db.get()?;

        self.save_changes::<Source>(&con).map_err(From::from)
    }
}

impl Source {
    /// Get the source `id` column.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// Represents the location(usually url) of the Feed xml file.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Set the `uri` field value.
    pub fn set_uri(&mut self, uri: String) {
        self.uri = uri;
    }

    /// Represents the Http Last-Modified Header field.
    ///
    /// See [RFC 7231](https://tools.ietf.org/html/rfc7231#section-7.2) for more.
    pub fn last_modified(&self) -> Option<&str> {
        self.last_modified.as_ref().map(|s| s.as_str())
    }

    /// Set `last_modified` value.
    pub fn set_last_modified(&mut self, value: Option<String>) {
        // self.last_modified = value.map(|x| x.to_string());
        self.last_modified = value;
    }

    /// Represents the Http Etag Header field.
    ///
    /// See [RFC 7231](https://tools.ietf.org/html/rfc7231#section-7.2) for more.
    pub fn http_etag(&self) -> Option<&str> {
        self.http_etag.as_ref().map(|s| s.as_str())
    }

    /// Set `http_etag` value.
    pub fn set_http_etag(&mut self, value: Option<&str>) {
        self.http_etag = value.map(|x| x.to_string());
    }

    /// Extract Etag and LastModifier from res, and update self and the
    /// corresponding db row.
    fn update_etag(mut self, res: &Response) -> Result<Self, DataError> {
        let headers = res.headers();

        let etag = headers.get::<ETag>().map(|x| x.tag());
        let lmod = headers.get::<LastModified>().map(|x| format!("{}", x));

        if (self.http_etag() != etag) || (self.last_modified != lmod) {
            self.set_http_etag(etag);
            self.set_last_modified(lmod);
            self = self.save()?;
        }

        Ok(self)
    }

    /// Clear the `HTTP` `Etag` and `Last-modified` headers.
    fn clear_etags(mut self) -> Result<Self, DataError> {
        if self.http_etag().is_some() || self.last_modified().is_some() {
            debug!("Source etags before clear: {:#?}", &self);
            self.http_etag = None;
            self.last_modified = None;
            self = self.save()?;
            debug!("Source etags after clear: {:#?}", &self);
            info!("Etag fields cleared succesfully for Source: {}", self.uri);
        }

        Ok(self)
    }

    fn make_err(self, context: &str, code: StatusCode) -> DataError {
        DataError::HttpStatusGeneral(HttpStatusError::new(self.uri, code, context.into()))
    }

    // TODO match on more stuff
    // 301: Moved Permanently
    // 304: Up to date Feed, checked with the Etag
    // 307: Temporary redirect of the url
    // 308: Permanent redirect of the url
    // 401: Unathorized
    // 403: Forbidden
    // 408: Timeout
    // 410: Feed deleted
    // TODO: Rething this api,
    fn match_status(mut self, res: Response) -> Result<Response, DataError> {
        let code = res.status();
        match code {
            // If request is succesful save the etag
            StatusCode::NotModified | StatusCode::Ok => self = self.update_etag(&res)?,
            // Clear the Etag/lmod else
            _ => self = self.clear_etags()?,
        };

        match code {
            StatusCode::NotModified => {
                info!("304: Source, (id: {}), is up to date", self.id());
                return Err(DataError::FeedNotModified(self));
            }
            StatusCode::MovedPermanently | StatusCode::Found | StatusCode::PermanentRedirect => {
                warn!("Feed was moved permanently.");
                self = self.update_url(&res)?;
                return Err(DataError::FeedRedirect(self));
            }
            StatusCode::TemporaryRedirect => {
                warn!("307: Temporary Redirect.");
                // FIXME: How is it actually handling the redirect?
                return Err(DataError::FeedRedirect(self));
            }
            StatusCode::Unauthorized => return Err(self.make_err("401: Unauthorized.", code)),
            StatusCode::Forbidden => return Err(self.make_err("403:  Forbidden.", code)),
            StatusCode::NotFound => return Err(self.make_err("404: Not found.", code)),
            StatusCode::RequestTimeout => return Err(self.make_err("408: Request Timeout.", code)),
            StatusCode::Gone => return Err(self.make_err("410: Feed was deleted..", code)),
            _ => info!("HTTP StatusCode: {}", code),
        };

        Ok(res)
    }

    fn update_url(mut self, res: &Response) -> Result<Self, DataError> {
        let code = res.status();
        let headers = res.headers();
        info!("HTTP StatusCode: {}", code);
        debug!("Headers {:#?}", headers);

        if let Some(url) = headers.get::<Location>() {
            debug!("Previous Source: {:#?}", &self);

            self.set_uri(url.to_string());
            self = self.clear_etags()?;

            debug!("Updated Source: {:#?}", &self);
            info!("Feed url of Source {}, was updated succesfully.", self.id());
        }

        Ok(self)
    }

    /// Construct a new `Source` with the given `uri` and index it.
    ///
    /// This only indexes the `Source` struct, not the Podcast Feed.
    pub fn from_url(uri: &str) -> Result<Source, DataError> {
        let url = Url::parse(uri)?;

        NewSource::new(&url).to_source()
    }

    /// `Feed` constructor.
    ///
    /// Fetches the latest xml Feed.
    ///
    /// Updates the validator Http Headers.
    ///
    /// Consumes `self` and Returns the corresponding `Feed` Object.
    // Refactor into TryInto once it lands on stable.
    pub fn into_feed(
        self,
        client: Client<HttpsConnector<HttpConnector>>,
    ) -> impl Future<Item = Feed, Error = DataError> {
        let id = self.id();
        let response = loop_fn(self, move |source| {
            source
                .request_constructor(&client.clone())
                .then(|res| match res {
                    Ok(response) => Ok(Loop::Break(response)),
                    Err(err) => match err {
                        DataError::FeedRedirect(s) => {
                            info!("Following redirect...");
                            Ok(Loop::Continue(s))
                        }
                        e => Err(e),
                    },
                })
        });

        response
            .and_then(response_to_channel)
            .and_then(move |chan| {
                FeedBuilder::default()
                    .channel(chan)
                    .source_id(id)
                    .build()
                    .map_err(From::from)
            })
    }

    fn request_constructor(
        self,
        client: &Client<HttpsConnector<HttpConnector>>,
    ) -> impl Future<Item = Response, Error = DataError> {
        // FIXME: remove unwrap somehow
        let uri = Uri::from_str(self.uri()).unwrap();
        let mut req = Request::new(Method::Get, uri);

        // Set the UserAgent cause ppl still seem to check it for some reason...
        req.headers_mut().set(UserAgent::new(USER_AGENT));

        if let Some(etag) = self.http_etag() {
            let tag = vec![EntityTag::new(true, etag.to_owned())];
            req.headers_mut().set(IfNoneMatch::Items(tag));
        }

        if let Some(lmod) = self.last_modified() {
            if let Ok(date) = lmod.parse::<HttpDate>() {
                req.headers_mut().set(IfModifiedSince(date));
            }
        }

        client
            .request(req)
            .map_err(From::from)
            .and_then(move |res| self.match_status(res))
    }
}

#[allow(needless_pass_by_value)]
fn response_to_channel(res: Response) -> impl Future<Item = Channel, Error = DataError> + Send {
    res.body()
        .concat2()
        .map(|x| x.into_iter())
        .map_err(From::from)
        .map(|iter| iter.collect::<Vec<u8>>())
        .map(|utf_8_bytes| String::from_utf8_lossy(&utf_8_bytes).into_owned())
        .and_then(|buf| Channel::from_str(&buf).map_err(From::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use failure::Error;
    use tokio_core::reactor::Core;

    use database::truncate_db;
    use utils::get_feed;

    #[test]
    fn test_into_feed() -> Result<(), Error> {
        truncate_db()?;

        let mut core = Core::new()?;
        let client = Client::configure()
            .connector(HttpsConnector::new(4, &core.handle())?)
            .build(&core.handle());

        let url = "https://web.archive.org/web/20180120083840if_/https://feeds.feedburner.\
                   com/InterceptedWithJeremyScahill";
        let source = Source::from_url(url)?;
        let id = source.id();

        let feed = source.into_feed(client);
        let feed = core.run(feed)?;

        let expected = get_feed("tests/feeds/2018-01-20-Intercepted.xml", id);
        assert_eq!(expected, feed);
        Ok(())
    }
}
