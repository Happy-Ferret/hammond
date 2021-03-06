use gtk;
use gtk::prelude::*;

use crossbeam_channel::Sender;
use failure::Error;

use app::Action;
use stacks::{HomeStack, ShowStack};

use std::cell::RefCell;
use std::rc::Rc;

use i18n::i18n;

#[derive(Debug, Clone, Copy)]
pub(crate) enum State {
    Populated,
    Empty,
}

#[derive(Debug, Clone)]
pub(crate) struct Content {
    stack: gtk::Stack,
    shows: Rc<RefCell<ShowStack>>,
    home: Rc<RefCell<HomeStack>>,
    sender: Sender<Action>,
}

impl Content {
    pub(crate) fn new(sender: &Sender<Action>) -> Result<Rc<Content>, Error> {
        let stack = gtk::Stack::new();
        let home = Rc::new(RefCell::new(HomeStack::new(sender.clone())?));
        let shows = Rc::new(RefCell::new(ShowStack::new(sender.clone())));

        stack.add_titled(&home.borrow().get_stack(), "home", &i18n("New"));
        stack.add_titled(&shows.borrow().get_stack(), "shows", &i18n("Shows"));

        let con = Content {
            stack,
            shows,
            home,
            sender: sender.clone(),
        };
        Ok(Rc::new(con))
    }

    pub(crate) fn update(&self) {
        self.update_home();
        self.update_shows();
    }

    pub(crate) fn update_home(&self) {
        self.home
            .borrow_mut()
            .update()
            .map_err(|err| error!("Failed to update HomeView: {}", err))
            .ok();
    }

    pub(crate) fn update_home_if_background(&self) {
        if self.stack.get_visible_child_name() != Some("home".into()) {
            self.update_home();
        }
    }

    fn update_shows(&self) {
        self.shows
            .borrow_mut()
            .update()
            .map_err(|err| error!("Failed to update ShowsView: {}", err))
            .ok();
    }

    pub(crate) fn update_shows_view(&self) {
        self.shows
            .borrow_mut()
            .update()
            .map_err(|err| error!("Failed to update ShowsView: {}", err))
            .ok();
    }

    pub(crate) fn update_widget_if_same(&self, pid: i32) {
        let pop = self.shows.borrow().populated();
        pop.borrow_mut()
            .update_widget_if_same(pid)
            .map_err(|err| error!("Failed to update ShowsWidget: {}", err))
            .ok();
    }

    pub(crate) fn get_stack(&self) -> gtk::Stack {
        self.stack.clone()
    }

    pub(crate) fn get_shows(&self) -> Rc<RefCell<ShowStack>> {
        self.shows.clone()
    }

    pub(crate) fn switch_to_empty_views(&self) {
        use gtk::StackTransitionType::*;

        self.home
            .borrow_mut()
            .switch_visible(State::Empty, Crossfade);
        self.shows.borrow_mut().switch_visible(State::Empty);
    }

    pub(crate) fn switch_to_populated(&self) {
        use gtk::StackTransitionType::*;

        self.home
            .borrow_mut()
            .switch_visible(State::Populated, Crossfade);
        self.shows.borrow_mut().switch_visible(State::Populated);
    }
}
