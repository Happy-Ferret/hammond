# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2018-02-11

* Tobias Bernard Redesigned the whole Gtk+ client.
* Complete re-write of hammond-data and hammond-gtk modules.
* Error handling for all crates was migrated from error-chain to Failure.
* Hammond-data now uses futures to parse feeds.
* Custom gtk-widgets are now composed structs as opposed to functions returning Gtk widgets.

## [0.2.0] - 2017-11-28

* Database Schema Breaking Changes.
* Added url sanitization. #4.
* Reworked and refactored of the hammond-data API.
* Added some more unit tests
* Documented hammond-data public API.

## [0.1.1] - 2017-11-13

* Added appdata.xml file

## [0.1.0] - 2017-11-13

Initial Release