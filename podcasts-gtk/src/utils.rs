#![cfg_attr(feature = "cargo-clippy", allow(type_complexity))]

use gdk::FrameClockExt;
use gdk_pixbuf::Pixbuf;
use glib;
use gtk;
use gtk::prelude::*;
use gtk::{IsA, Widget};

use chrono::prelude::*;
use crossbeam_channel::{unbounded, Sender};
use failure::Error;
use fragile::Fragile;
use rayon;
use regex::Regex;
use reqwest;
use serde_json::Value;

// use podcasts_data::feed;
use podcasts_data::dbqueries;
use podcasts_data::opml;
use podcasts_data::pipeline;
use podcasts_data::utils::checkup;
use podcasts_data::Source;
use podcasts_downloader::downloader;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use app::Action;

/// Lazy evaluates and loads widgets to the parent `container` widget.
///
/// Accepts an `IntoIterator`, `data`, as the source from which each widget
/// will be constructed. An `FnMut` function that returns the desired
/// widget should be passed as the widget `constructor`. You can also specify
/// a `callback` that will be executed when the iteration finish.
///
/// ```no_run
/// # struct Message;
/// # struct MessageWidget(gtk::Label);
///
/// # impl MessageWidget {
/// #    fn new(_: Message) -> Self {
/// #        MessageWidget(gtk::Label::new("A message"))
/// #    }
/// # }
///
/// let messages: Vec<Message> = Vec::new();
/// let list = gtk::ListBox::new();
/// let constructor = |m| MessageWidget::new(m).0;
/// lazy_load(messages, list, constructor, || {});
/// ```
///
/// If you have already constructed the widgets and only want to
/// load them to the parent you can pass a closure that returns it's
/// own argument to the constructor.
///
/// ```no_run
/// # use std::collections::binary_heap::BinaryHeap;
/// let widgets: BinaryHeap<gtk::Button> = BinaryHeap::new();
/// let list = gtk::ListBox::new();
/// lazy_load(widgets, list, |w| w, || {});
/// ```
pub fn lazy_load<T, C, F, W, U>(data: T, container: C, mut contructor: F, callback: U)
where
    T: IntoIterator + 'static,
    T::Item: 'static,
    C: ContainerExt + 'static,
    F: FnMut(T::Item) -> W + 'static,
    W: IsA<Widget> + WidgetExt,
    U: Fn() + 'static,
{
    let func = move |x| {
        let widget = contructor(x);
        container.add(&widget);
        widget.show();
    };
    lazy_load_full(data, func, callback);
}

/// Iterate over `data` and execute `func` using a `gtk::idle_add()`,
/// when the iteration finishes, it executes `finish_callback`.
///
/// This is a more flexible version of `lazy_load` with less constrains.
/// If you just want to lazy add `widgets` to a `container` check if
/// `lazy_load` fits your needs first.
#[cfg_attr(feature = "cargo-clippy", allow(redundant_closure))]
pub fn lazy_load_full<T, F, U>(data: T, mut func: F, finish_callback: U)
where
    T: IntoIterator + 'static,
    T::Item: 'static,
    F: FnMut(T::Item) + 'static,
    U: Fn() + 'static,
{
    let mut data = data.into_iter();
    gtk::idle_add(move || {
        data.next()
            .map(|x| func(x))
            .map(|_| glib::Continue(true))
            .unwrap_or_else(|| {
                finish_callback();
                glib::Continue(false)
            })
    });
}

// Kudos to Julian Sparber
// https://blogs.gnome.org/jsparber/2018/04/29/animate-a-scrolledwindow/
#[cfg_attr(feature = "cargo-clippy", allow(float_cmp))]
pub fn smooth_scroll_to(view: &gtk::ScrolledWindow, target: &gtk::Adjustment) {
    if let Some(adj) = view.get_vadjustment() {
        if let Some(clock) = view.get_frame_clock() {
            let duration = 200;
            let start = adj.get_value();
            let end = target.get_value();
            let start_time = clock.get_frame_time();
            let end_time = start_time + 1000 * duration;

            view.add_tick_callback(move |_, clock| {
                let now = clock.get_frame_time();
                // FIXME: `adj.get_value != end` is a float comparison...
                if now < end_time && adj.get_value().abs() != end.abs() {
                    let mut t = (now - start_time) as f64 / (end_time - start_time) as f64;
                    t = ease_out_cubic(t);
                    adj.set_value(start + t * (end - start));
                    glib::Continue(true)
                } else {
                    adj.set_value(end);
                    glib::Continue(false)
                }
            });
        }
    }
}

// From clutter-easing.c, based on Robert Penner's
// infamous easing equations, MIT license.
fn ease_out_cubic(t: f64) -> f64 {
    let p = t - 1f64;
    p * p * p + 1f64
}

lazy_static! {
    static ref IGNORESHOWS: Arc<Mutex<HashSet<i32>>> = Arc::new(Mutex::new(HashSet::new()));
}

pub fn ignore_show(id: i32) -> Result<bool, Error> {
    IGNORESHOWS
        .lock()
        .map(|mut guard| guard.insert(id))
        .map_err(|err| format_err!("{}", err))
}

pub fn uningore_show(id: i32) -> Result<bool, Error> {
    IGNORESHOWS
        .lock()
        .map(|mut guard| guard.remove(&id))
        .map_err(|err| format_err!("{}", err))
}

pub fn get_ignored_shows() -> Result<Vec<i32>, Error> {
    IGNORESHOWS
        .lock()
        .map(|guard| guard.iter().cloned().collect::<Vec<_>>())
        .map_err(|err| format_err!("{}", err))
}

pub fn cleanup(cleanup_date: DateTime<Utc>) {
    checkup(cleanup_date)
        .map_err(|err| error!("Check up failed: {}", err))
        .ok();
}

pub fn refresh<S>(source: Option<S>, sender: Sender<Action>)
where
    S: IntoIterator<Item = Source> + Send + 'static,
{
    refresh_feed(source, sender)
        .map_err(|err| error!("Failed to update feeds: {}", err))
        .ok();
}

/// Update the rss feed(s) originating from `source`.
/// If `source` is None, Fetches all the `Source` entries in the database and updates them.
/// When It's done,it queues up a `RefreshViews` action.
fn refresh_feed<S>(source: Option<S>, sender: Sender<Action>) -> Result<(), Error>
where
    S: IntoIterator<Item = Source> + Send + 'static,
{
    sender.send(Action::HeaderBarShowUpdateIndicator);

    rayon::spawn(move || {
        if let Some(s) = source {
            // Refresh only specified feeds
            pipeline::run(s, false)
                .map_err(|err| error!("Error: {}", err))
                .map_err(|_| error!("Error While trying to update the database."))
                .ok();
        } else {
            // Refresh all the feeds
            dbqueries::get_sources()
                .map(|s| s.into_iter())
                .and_then(|s| pipeline::run(s, false))
                .map_err(|err| error!("Error: {}", err))
                .ok();
        };

        sender.send(Action::HeaderBarHideUpdateIndicator);
        sender.send(Action::RefreshAllViews);
    });
    Ok(())
}

lazy_static! {
    static ref CACHED_PIXBUFS: RwLock<HashMap<(i32, u32), Mutex<Fragile<Pixbuf>>>> =
        { RwLock::new(HashMap::new()) };
    static ref COVER_DL_REGISTRY: RwLock<HashSet<i32>> = RwLock::new(HashSet::new());
    static ref THREADPOOL: rayon::ThreadPool = rayon::ThreadPoolBuilder::new().build().unwrap();
}

// Since gdk_pixbuf::Pixbuf is refference counted and every episode,
// use the cover of the Podcast Feed/Show, We can only create a Pixbuf
// cover per show and pass around the Rc pointer.
//
// GObjects do not implement Send trait, so SendCell is a way around that.
// Also lazy_static requires Sync trait, so that's what the mutexes are.
// TODO: maybe use something that would just scale to requested size?
pub fn set_image_from_path(image: &gtk::Image, show_id: i32, size: u32) -> Result<(), Error> {
    // Check if there's an active download about this show cover.
    // If there is, a callback will be set so this function will be called again.
    // If the download succedes, there should be a quick return from the pixbuf cache_image
    // If it fails another download will be scheduled.
    if let Ok(guard) = COVER_DL_REGISTRY.read() {
        if guard.contains(&show_id) {
            let callback = clone!(image => move || {
                 let _ = set_image_from_path(&image, show_id, size);
                 glib::Continue(false)
            });
            gtk::timeout_add(250, callback);
            return Ok(());
        }
    }

    if let Ok(hashmap) = CACHED_PIXBUFS.read() {
        // Check if the requested (cover + size) is already in the chache
        // and if so do an early return after that.
        if let Some(guard) = hashmap.get(&(show_id, size)) {
            guard
                .lock()
                .map_err(|err| format_err!("Fragile Mutex: {}", err))
                .and_then(|fragile| {
                    fragile
                        .try_get()
                        .map(|px| image.set_from_pixbuf(px))
                        .map_err(From::from)
                })?;

            return Ok(());
        }
    }

    let (sender, receiver) = unbounded();
    THREADPOOL.spawn(move || {
        if let Ok(mut guard) = COVER_DL_REGISTRY.write() {
            guard.insert(show_id);
            if let Ok(pd) = dbqueries::get_podcast_cover_from_id(show_id) {
                sender.send(downloader::cache_image(&pd));
            }
            guard.remove(&show_id);
        }
    });

    let image = image.clone();
    let s = size as i32;
    gtk::timeout_add(25, move || {
        if let Some(path) = receiver.try_recv() {
            if let Ok(path) = path {
                if let Ok(px) = Pixbuf::new_from_file_at_scale(&path, s, s, true) {
                    if let Ok(mut hashmap) = CACHED_PIXBUFS.write() {
                        hashmap.insert((show_id, size), Mutex::new(Fragile::new(px.clone())));
                        image.set_from_pixbuf(&px);
                    }
                }
            }
            glib::Continue(false)
        } else {
            glib::Continue(true)
        }
    });
    Ok(())
}

// FIXME: the signature should be `fn foo(s: Url) -> Result<Url, Error>`
pub fn itunes_to_rss(url: &str) -> Result<String, Error> {
    let id = itunes_id_from_url(url).ok_or_else(|| format_err!("Failed to find an Itunes ID."))?;
    lookup_id(id)
}

fn itunes_id_from_url(url: &str) -> Option<u32> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"/id([0-9]+)").unwrap();
    }

    // Get the itunes id from the url
    let foo = RE.captures_iter(url).nth(0)?.get(1)?.as_str();
    // Parse it to a u32, this *should* never fail
    foo.parse::<u32>().ok()
}

fn lookup_id(id: u32) -> Result<String, Error> {
    let url = format!("https://itunes.apple.com/lookup?id={}&entity=podcast", id);
    let req: Value = reqwest::get(&url)?.json()?;
    let rssurl = || -> Option<&str> { req.get("results")?.get(0)?.get("feedUrl")?.as_str() };
    rssurl()
        .map(From::from)
        .ok_or_else(|| format_err!("Failed to get url from itunes response"))
}

pub fn on_import_clicked(window: &gtk::ApplicationWindow, sender: &Sender<Action>) {
    use glib::translate::ToGlib;
    use gtk::{FileChooserAction, FileChooserNative, FileFilter, ResponseType};

    // let dialog = FileChooserDialog::new(title, Some(&window), FileChooserAction::Open);
    // TODO: It might be better to use a FileChooserNative widget.
    // Create the FileChooser Dialog
    let dialog = FileChooserNative::new(
        Some("Select the file from which to you want to Import Shows."),
        Some(window),
        FileChooserAction::Open,
        Some("_Import"),
        None,
    );

    // Do not show hidden(.thing) files
    dialog.set_show_hidden(false);

    // Set a filter to show only xml files
    let filter = FileFilter::new();
    FileFilterExt::set_name(&filter, Some("OPML file"));
    filter.add_mime_type("application/xml");
    filter.add_mime_type("text/xml");
    dialog.add_filter(&filter);

    dialog.connect_response(clone!(sender => move |dialog, resp| {
        debug!("Dialong Response {}", resp);
        if resp == ResponseType::Accept.to_glib() {
            // TODO: Show an in-app notifictaion if the file can not be accessed
            if let Some(filename) = dialog.get_filename() {
                debug!("File selected: {:?}", filename);

                rayon::spawn(clone!(sender => move || {
                    // Parse the file and import the feeds
                    if let Ok(sources) = opml::import_from_file(filename) {
                        // Refresh the succesfully parsed feeds to index them
                        refresh(Some(sources), sender)
                    } else {
                        let text = String::from("Failed to parse the Imported file");
                        sender.send(Action::ErrorNotification(text));
                    }
                }))
            } else {
                let text = String::from("Selected File could not be accessed.");
                sender.send(Action::ErrorNotification(text));
            }
        }

        dialog.destroy();
    }));

    dialog.run();
}

#[cfg(test)]
mod tests {
    use super::*;
    // use podcasts_data::Source;
    // use podcasts_data::dbqueries;

    // #[test]
    // This test inserts an rss feed to your `XDG_DATA/podcasts/podcasts.db` so we make it explicit
    // to run it.
    // #[ignore]
    // Disabled till https://gitlab.gnome.org/World/podcasts/issues/56
    // fn test_set_image_from_path() {
    //     let url = "https://web.archive.org/web/20180120110727if_/https://rss.acast.com/thetipoff";
    // Create and index a source
    //     let source = Source::from_url(url).unwrap();
    // Copy it's id
    //     let sid = source.id();
    //     pipeline::run(vec![source], true).unwrap();

    // Get the Podcast
    //     let img = gtk::Image::new();
    //     let pd = dbqueries::get_podcast_from_source_id(sid).unwrap().into();
    //     let pxbuf = set_image_from_path(&img, Arc::new(pd), 256);
    //     assert!(pxbuf.is_ok());
    // }

    #[test]
    fn test_itunes_to_rss() {
        let itunes_url = "https://itunes.apple.com/podcast/id1195206601";
        let rss_url = String::from("http://feeds.feedburner.com/InterceptedWithJeremyScahill");
        assert_eq!(rss_url, itunes_to_rss(itunes_url).unwrap());

        let itunes_url = "https://itunes.apple.com/podcast/id000000000000000";
        assert!(itunes_to_rss(itunes_url).is_err());
    }

    #[test]
    fn test_itunes_id() {
        let id = 1195206601;
        let itunes_url = "https://itunes.apple.com/podcast/id1195206601";
        assert_eq!(id, itunes_id_from_url(itunes_url).unwrap());
    }

    #[test]
    fn test_itunes_lookup_id() {
        let id = 1195206601;
        let rss_url = "http://feeds.feedburner.com/InterceptedWithJeremyScahill";
        assert_eq!(rss_url, lookup_id(id).unwrap());

        let id = 000000000;
        assert!(lookup_id(id).is_err());
    }
}