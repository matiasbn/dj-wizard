use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SoundeoAPITrack {
    id: String,
    title: String,
    cover: String,
    track_url: String,
    release: String,
    release_url: String,
    label: String,
    label_url: String,
    genre: String,
    genre_url: String,
    date: String,
    bpm: String,
    bpm_url: String,
    key: String,
    key_url: String,
    downloadable: bool,
    downloaded_f1: bool,
    downloaded_f2: bool,
    format1: u32,
    format1str: String,
    format1size: String,
    format2: u32,
    format2str: String,
    format2size: String,
    favored: bool,
    voteable: bool,
    voteable_by_user: bool,
    votes: String,
    restricted: bool,
    broken: bool,
    in_progress: bool,
    replacement: bool,
}

impl SoundeoAPITrack {
    fn new(id: String) -> Self {
        SoundeoAPITrack {
            id,
            title: String::new(),
            cover: String::new(),
            track_url: String::new(),
            release: String::new(),
            release_url: String::new(),
            label: String::new(),
            label_url: String::new(),
            genre: String::new(),
            genre_url: String::new(),
            date: String::new(),
            bpm: String::new(),
            bpm_url: String::new(),
            key: String::new(),
            key_url: String::new(),
            downloadable: false,
            downloaded_f1: false,
            downloaded_f2: false,
            format1: 0,
            format1str: String::new(),
            format1size: String::new(),
            format2: 0,
            format2str: String::new(),
            format2size: String::new(),
            favored: false,
            voteable: false,
            voteable_by_user: false,
            votes: String::new(),
            restricted: false,
            broken: false,
            in_progress: false,
            replacement: false,
        }
    }
}

mod api {
    async fn get_track_info_from_track_id(track_id: String) {}
}
