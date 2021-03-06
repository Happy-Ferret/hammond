use gtk::{self, prelude::*, Adjustment, Align, SelectionMode};

use crossbeam_channel::Sender;
use failure::Error;

use podcasts_data::dbqueries;
use podcasts_data::Show;

use app::Action;
use utils::{get_ignored_shows, lazy_load, set_image_from_path};
use widgets::BaseView;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct ShowsView {
    pub(crate) view: BaseView,
    flowbox: gtk::FlowBox,
}

impl Default for ShowsView {
    fn default() -> Self {
        let view = BaseView::default();
        let flowbox = gtk::FlowBox::new();

        flowbox.show();
        flowbox.set_vexpand(true);
        flowbox.set_hexpand(true);
        flowbox.set_row_spacing(12);
        flowbox.set_can_focus(false);
        flowbox.set_margin_top(32);
        flowbox.set_margin_bottom(32);
        flowbox.set_homogeneous(true);
        flowbox.set_column_spacing(12);
        flowbox.set_valign(Align::Start);
        flowbox.set_halign(Align::Center);
        flowbox.set_selection_mode(SelectionMode::None);
        view.add(&flowbox);

        ShowsView { view, flowbox }
    }
}

impl ShowsView {
    pub(crate) fn new(sender: Sender<Action>, vadj: Option<Adjustment>) -> Rc<Self> {
        let pop = Rc::new(ShowsView::default());
        pop.init(sender);
        // Populate the flowbox with the Shows.
        let res = populate_flowbox(&pop, vadj);
        debug_assert!(res.is_ok());
        pop
    }

    pub(crate) fn init(&self, sender: Sender<Action>) {
        self.flowbox.connect_child_activated(move |_, child| {
            let res = on_child_activate(child, &sender);
            debug_assert!(res.is_ok());
        });
    }
}

fn populate_flowbox(shows: &Rc<ShowsView>, vadj: Option<Adjustment>) -> Result<(), Error> {
    let ignore = get_ignored_shows()?;
    let podcasts = dbqueries::get_podcasts_filter(&ignore)?;
    let show_weak = Rc::downgrade(&shows);
    let flowbox_weak = shows.flowbox.downgrade();

    let constructor = move |parent| ShowsChild::new(&parent).child;
    let callback = move || {
        match (show_weak.upgrade(), &vadj) {
            (Some(ref shows), Some(ref v)) => shows.view.set_adjutments(None, Some(v)),
            _ => (),
        };
    };

    lazy_load(podcasts, flowbox_weak, constructor, callback);
    Ok(())
}

fn on_child_activate(child: &gtk::FlowBoxChild, sender: &Sender<Action>) -> Result<(), Error> {
    use gtk::WidgetExt;

    // This is such an ugly hack...
    let id = WidgetExt::get_name(child)
        .ok_or_else(|| format_err!("Failed to get \"episodes\" child from the stack."))?
        .parse::<i32>()?;
    let pd = Arc::new(dbqueries::get_podcast_from_id(id)?);

    sender.send(Action::HeaderBarShowTile(pd.title().into()));
    sender.send(Action::ReplaceWidget(pd));
    sender.send(Action::ShowWidgetAnimated);
    Ok(())
}

#[derive(Debug, Clone)]
struct ShowsChild {
    cover: gtk::Image,
    child: gtk::FlowBoxChild,
}

impl Default for ShowsChild {
    fn default() -> Self {
        let cover = gtk::Image::new_from_icon_name("image-x-generic-symbolic", -1);
        let child = gtk::FlowBoxChild::new();

        cover.set_pixel_size(256);
        child.add(&cover);
        child.show_all();

        ShowsChild { cover, child }
    }
}

impl ShowsChild {
    pub(crate) fn new(pd: &Show) -> ShowsChild {
        let child = ShowsChild::default();
        child.init(pd);
        child
    }

    fn init(&self, pd: &Show) {
        self.child.set_tooltip_text(pd.title());
        WidgetExt::set_name(&self.child, &pd.id().to_string());

        self.set_cover(pd.id())
    }

    fn set_cover(&self, show_id: i32) {
        // The closure above is a regular `Fn` closure.
        // which means we can't mutate stuff inside it easily,
        // so Cell is used.
        //
        // `Option<T>` along with the `.take()` method ensure
        // that the function will only be run once, during the first execution.
        let show_id = Cell::new(Some(show_id));

        self.cover.connect_draw(move |cover, _| {
            show_id.take().map(|id| {
                set_image_from_path(cover, id, 256)
                    .map_err(|err| error!("Failed to set a cover: {}", err))
                    .ok();
            });

            gtk::Inhibit(false)
        });
    }
}
