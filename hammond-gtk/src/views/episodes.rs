use chrono::prelude::*;
use gtk;
use gtk::prelude::*;

use hammond_data::EpisodeWidgetQuery;
use hammond_data::dbqueries;

use app::Action;
use utils::get_pixbuf_from_path;
use widgets::episode::EpisodeWidget;

use std::sync::mpsc::Sender;

#[derive(Debug, Clone)]
enum ListSplit {
    Today,
    Yday,
    Week,
    Month,
    Rest,
}

#[derive(Debug, Clone)]
pub struct EpisodesView {
    pub container: gtk::Box,
    scrolled_window: gtk::ScrolledWindow,
    frame_parent: gtk::Box,
    today_box: gtk::Box,
    yday_box: gtk::Box,
    week_box: gtk::Box,
    month_box: gtk::Box,
    rest_box: gtk::Box,
    today_list: gtk::ListBox,
    yday_list: gtk::ListBox,
    week_list: gtk::ListBox,
    month_list: gtk::ListBox,
    rest_list: gtk::ListBox,
}

impl Default for EpisodesView {
    fn default() -> Self {
        let builder = gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/episodes_view.ui");
        let container: gtk::Box = builder.get_object("container").unwrap();
        let scrolled_window: gtk::ScrolledWindow = builder.get_object("scrolled_window").unwrap();
        let frame_parent: gtk::Box = builder.get_object("frame_parent").unwrap();
        let today_box: gtk::Box = builder.get_object("today_box").unwrap();
        let yday_box: gtk::Box = builder.get_object("yday_box").unwrap();
        let week_box: gtk::Box = builder.get_object("week_box").unwrap();
        let month_box: gtk::Box = builder.get_object("month_box").unwrap();
        let rest_box: gtk::Box = builder.get_object("rest_box").unwrap();
        let today_list: gtk::ListBox = builder.get_object("today_list").unwrap();
        let yday_list: gtk::ListBox = builder.get_object("yday_list").unwrap();
        let week_list: gtk::ListBox = builder.get_object("week_list").unwrap();
        let month_list: gtk::ListBox = builder.get_object("month_list").unwrap();
        let rest_list: gtk::ListBox = builder.get_object("rest_list").unwrap();

        EpisodesView {
            container,
            scrolled_window,
            frame_parent,
            today_box,
            yday_box,
            week_box,
            month_box,
            rest_box,
            today_list,
            yday_list,
            week_list,
            month_list,
            rest_list,
        }
    }
}

// TODO: REFACTOR ME
impl EpisodesView {
    pub fn new(sender: Sender<Action>) -> EpisodesView {
        let view = EpisodesView::default();
        let episodes = dbqueries::get_episodes_widgets_with_limit(50).unwrap();
        let now_utc = Utc::now();

        episodes.into_iter().for_each(|mut ep| {
            let viewep = EpisodesViewWidget::new(&mut ep, sender.clone());

            let t = split(&now_utc, i64::from(ep.epoch()));
            match t {
                ListSplit::Today => {
                    view.today_list.add(&viewep.container);
                }
                ListSplit::Yday => {
                    view.yday_list.add(&viewep.container);
                }
                ListSplit::Week => {
                    view.week_list.add(&viewep.container);
                }
                ListSplit::Month => {
                    view.month_list.add(&viewep.container);
                }
                ListSplit::Rest => {
                    view.rest_list.add(&viewep.container);
                }
            }
        });

        if view.today_list.get_children().is_empty() {
            view.today_box.hide();
        }

        if view.yday_list.get_children().is_empty() {
            view.yday_box.hide();
        }

        if view.week_list.get_children().is_empty() {
            view.week_box.hide();
        }

        if view.month_list.get_children().is_empty() {
            view.month_box.hide();
        }

        if view.rest_list.get_children().is_empty() {
            view.rest_box.hide();
        }

        view.container.show_all();
        view
    }

    pub fn is_empty(&self) -> bool {
        if !self.today_list.get_children().is_empty() {
            return false;
        }

        if !self.yday_list.get_children().is_empty() {
            return false;
        }

        if !self.week_list.get_children().is_empty() {
            return false;
        }

        if !self.month_list.get_children().is_empty() {
            return false;
        }

        if !self.rest_list.get_children().is_empty() {
            return false;
        }

        true
    }

    /// Set scrolled window vertical adjustment.
    pub fn set_vadjustment(&self, vadjustment: &gtk::Adjustment) {
        self.scrolled_window.set_vadjustment(vadjustment)
    }
}

fn split(now: &DateTime<Utc>, epoch: i64) -> ListSplit {
    let ep = Utc.timestamp(epoch, 0);

    if now.ordinal() == ep.ordinal() && now.year() == ep.year() {
        ListSplit::Today
    } else if now.ordinal() == ep.ordinal() + 1 && now.year() == ep.year() {
        ListSplit::Yday
    } else if now.iso_week().week() == ep.iso_week().week() && now.year() == ep.year() {
        ListSplit::Week
    } else if now.month() == ep.month() && now.year() == ep.year() {
        ListSplit::Month
    } else {
        ListSplit::Rest
    }
}

#[derive(Debug, Clone)]
struct EpisodesViewWidget {
    container: gtk::Box,
    image: gtk::Image,
    episode: gtk::Box,
}

impl Default for EpisodesViewWidget {
    fn default() -> Self {
        let builder =
            gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/episodes_view_widget.ui");
        let container: gtk::Box = builder.get_object("container").unwrap();
        let image: gtk::Image = builder.get_object("cover").unwrap();
        let ep = EpisodeWidget::default();
        container.pack_start(&ep.container, true, true, 6);

        EpisodesViewWidget {
            container,
            image,
            episode: ep.container,
        }
    }
}

impl EpisodesViewWidget {
    fn new(episode: &mut EpisodeWidgetQuery, sender: Sender<Action>) -> EpisodesViewWidget {
        let builder =
            gtk::Builder::new_from_resource("/org/gnome/hammond/gtk/episodes_view_widget.ui");
        let container: gtk::Box = builder.get_object("container").unwrap();
        let image: gtk::Image = builder.get_object("cover").unwrap();

        if let Ok(pd) = dbqueries::get_podcast_cover_from_id(episode.podcast_id()) {
            get_pixbuf_from_path(&pd, 64).map(|img| image.set_from_pixbuf(&img));
        }

        let ep = EpisodeWidget::new(episode, sender.clone());
        container.pack_start(&ep.container, true, true, 6);

        EpisodesViewWidget {
            container,
            image,
            episode: ep.container,
        }
    }
}
