use reqwest;
use hyper::header::*;
use tempdir::TempDir;
use rand;
use rand::Rng;
use mime_guess;

use std::fs::{rename, DirBuilder, File};
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use errors::*;
use hammond_data::index_feed::Database;
use hammond_data::models::{Episode, Podcast};
use hammond_data::{DL_DIR, HAMMOND_CACHE};

// TODO: Replace path that are of type &str with std::path.
// TODO: Have a convention/document absolute/relative paths, if they should end with / or not.

// Adapted from https://github.com/mattgathu/rget .
// I never wanted to write a custom downloader.
// Sorry to those who will have to work with that code.
// Would much rather use a crate,
// or bindings for a lib like youtube-dl(python),
// But cant seem to find one.
// TODO: Write unit-tests.
fn download_into(dir: &str, file_title: &str, url: &str) -> Result<String> {
    info!("GET request to: {}", url);
    let client = reqwest::Client::builder().referer(false).build()?;
    let mut resp = client.get(url).send()?;
    info!("Status Resp: {}", resp.status());

    if !resp.status().is_success() {
        // TODO: Return an error instead of panicking.
        panic!("Bad request response.");
    }

    let headers = resp.headers().clone();

    let ct_len = headers.get::<ContentLength>().map(|ct_len| **ct_len);
    let ct_type = headers.get::<ContentType>();
    ct_len.map(|x| info!("File Lenght: {}", x));
    ct_type.map(|x| info!("Content Type: {}", x));

    // This could be prettier.
    // Determine the file extension from the http content-type header.
    let ext = if let Some(t) = ct_type {
        let mime = mime_guess::get_extensions(t.type_().as_ref(), t.subtype().as_ref());
        if let Some(m) = mime {
            if m.contains(&t.subtype().as_ref()) {
                t.subtype().as_ref().to_string()
            } else {
                m.first().unwrap().to_string()
            }
        } else {
            error!("Unkown mime type. {}", t);
            "unkown".to_string()
        }
    } else {
        error!("Unkown mime type.");
        "unkown".to_string()
    };
    info!("Extension: {}", ext);

    // Construct a temp file to save desired content.
    let tempdir = TempDir::new_in(dir, "")?;
    let mut rng = rand::thread_rng();

    let out_file = format!(
        "{}/{}.part",
        tempdir.path().to_str().unwrap(),
        rng.gen::<usize>()
    );

    // Save requested content into the file.
    save_io(&out_file, &mut resp, ct_len)?;

    // Construct the desired path.
    let target = format!("{}/{}.{}", dir, file_title, ext);
    // Rename/move the tempfile into a permanent place upon success.
    rename(out_file, &target)?;
    info!("Downloading of {} completed succesfully.", &target);
    Ok(target)
}

// TODO: Write unit-tests.
/// Handles the I/O of fetching a remote file and saving into a Buffer and A File.
fn save_io(file: &str, resp: &mut reqwest::Response, content_lenght: Option<u64>) -> Result<()> {
    info!("Downloading into: {}", file);
    let chunk_size = match content_lenght {
        Some(x) => x as usize / 99,
        None => 1024 as usize, // default chunk size
    };

    let mut writer = BufWriter::new(File::create(&file)?);

    loop {
        let mut buffer = vec![0; chunk_size];
        let bcount = resp.read(&mut buffer[..])?;
        buffer.truncate(bcount);
        if !buffer.is_empty() {
            writer.write_all(buffer.as_slice())?;
        } else {
            break;
        }
    }

    Ok(())
}

pub fn get_download_folder(pd_title: &str) -> Result<String> {
    // It might be better to make it a hash of the title
    let download_fold = format!("{}/{}", DL_DIR.to_str().unwrap(), pd_title);

    // Create the folder
    DirBuilder::new().recursive(true).create(&download_fold)?;
    Ok(download_fold)
}

// TODO: Refactor
pub fn get_episode(connection: &Database, ep: &mut Episode, download_folder: &str) -> Result<()> {
    // Check if its alrdy downloaded
    if ep.local_uri().is_some() {
        if Path::new(ep.local_uri().unwrap()).exists() {
            return Ok(());
        }

        // If the path is not valid, then set it to None.
        ep.set_local_uri(None);
        ep.save(connection)?;
    };

    let res = download_into(download_folder, ep.title().unwrap(), ep.uri());

    if let Ok(path) = res {
        // If download succedes set episode local_uri to dlpath.
        ep.set_local_uri(Some(&path));
        ep.save(connection)?;
        Ok(())
    } else {
        error!("Something whent wrong while downloading.");
        Err(res.unwrap_err())
    }
}

pub fn cache_image(pd: &Podcast) -> Option<String> {
    if pd.image_uri().is_none() {
        return None;
    }

    let url = pd.image_uri().unwrap().to_owned();
    if url == "" {
        return None;
    }

    let download_fold = format!(
        "{}{}",
        HAMMOND_CACHE.to_str().unwrap(),
        pd.title().to_owned()
    );

    // Hacky way
    // TODO: make it so it returns the first cover.* file encountered.
    let png = format!("{}/cover.png", download_fold);
    let jpg = format!("{}/cover.jpg", download_fold);
    let jpe = format!("{}/cover.jpe", download_fold);
    let jpeg = format!("{}/cover.jpeg", download_fold);
    if Path::new(&png).exists() {
        return Some(png);
    } else if Path::new(&jpe).exists() {
        return Some(jpe);
    } else if Path::new(&jpg).exists() {
        return Some(jpg);
    } else if Path::new(&jpeg).exists() {
        return Some(jpeg);
    };

    DirBuilder::new()
        .recursive(true)
        .create(&download_fold)
        .unwrap();

    let dlpath = download_into(&download_fold, "cover", &url);
    if let Ok(path) = dlpath {
        info!("Cached img into: {}", &path);
        Some(path)
    } else {
        error!("Failed to get feed image.");
        error!("Error: {}", dlpath.unwrap_err());
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hammond_data::{DL_DIR, HAMMOND_CACHE};
    use hammond_data::models::NewPodcast;

    use std::fs;

    #[test]
    fn test_get_dl_folder() {
        let foo_ = format!("{}/{}", DL_DIR.to_str().unwrap(), "foo");
        assert_eq!(get_download_folder("foo").unwrap(), foo_);
        let _ = fs::remove_dir_all(foo_);
    }

    #[test]
    fn test_cache_image() {
        let pd = NewPodcast {
            title: "New Rustacean".to_string(),
            description: "".to_string(),
            link: "".to_string(),
            image_uri: Some("http://newrustacean.com/podcast.png".to_string()),
            source_id: 0,
        };
        let pd = Podcast::from(pd);
        let img_path = cache_image(&pd);
        let foo_ = format!(
            "{}{}/cover.png",
            HAMMOND_CACHE.to_str().unwrap(),
            "New Rustacean"
        );
        assert_eq!(img_path, Some(foo_));
    }
}
