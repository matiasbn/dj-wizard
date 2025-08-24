# dj-wizard Command Options

This document outlines the available commands and interactive options for the `dj-wizard` CLI application based on its source code.

## `dj-wizard login`

Stores Soundeo.com credentials and the desired download directory.

* Prompts for:
    * Soundeo username/email.
    * Soundeo password (with confirmation).
    * Download directory path (opens a folder selection dialog).
* Saves the information to the configuration file (`~/.dj_wizard_config/config.json`).

## `dj-wizard ipfs`

Manages interaction with IPFS for uploading the application's log file.

* Presents an interactive menu:
    * **Upload log to IPFS:** Reads the current IPFS credentials from the config, uploads `soundeo_log.json` to Infura IPFS, and stores the resulting hash back in the config file.
    * **Update IPFS credentials:** Prompts for:
        * IPFS API Key (e.g., from Infura).
        * IPFS API Key Secret (with confirmation).
        * Saves the credentials to the configuration file.

## `dj-wizard config`

Reads and displays the current contents of the configuration file (`~/.dj_wizard_config/config.json`), showing stored credentials and paths.

## `dj-wizard queue`

Manages the track download queue.

* **Without Flags:** (`dj-wizard queue`)
    * Presents an interactive menu with the following options:
        * **Add To Queue From Url:**
            * Prompts for a Soundeo URL (e.g., chart, label, search result).
            * Scrapes track IDs from the URL.
            * Asks if already downloaded tracks (according to the log) should be queued again (Yes/No).
            * Adds track IDs to the "queued" list in `soundeo_log.json`.
        * **Add To Queue From Url List:**
            * Reads the list of saved Soundeo URLs from `soundeo_log.json`.
            * Asks if already downloaded tracks should be queued again (Yes/No).
            * Processes each URL, scraping track IDs and adding them to the queue.
            * Removes successfully processed URLs from the saved list.
        * **Resume Queue:**
            * Asks if you want to filter the queue by genre before processing (Yes/No).
            * If Yes: Displays genres present in the queue and prompts to select one.
            * Processes the selected tracks (either filtered or the full queue): Attempts to get download links for queued tracks (moving them to the "available" list in the log if successful), then attempts to download tracks from the "available" list. Handles download limits.
        * **Save To Available Tracks:**
            * Prompts for a Soundeo URL.
            * Scrapes track IDs from the URL.
            * Asks if already downloaded tracks should be processed again (Yes/No).
            * For each track ID, attempts to get the download link from Soundeo (which adds it to the user's collection on the site). If successful, adds the track ID to the "available" list in the log. If getting the link fails (e.g., download limit), adds it to the "queued" list.
        * **Download Only Available Tracks:**
            * Reads the "available" track list from the log.
            * Attempts to download each track.
            * Removes successfully downloaded tracks from the "available" list.
        * **Get Queue Info:**
            * Reads the "queued" track list from the log.
            * Prints a summary of queued tracks, grouped by genre, including counts.
* **With `-r` or `--resume-queue` Flag:** (`dj-wizard queue -r`)
    * Directly executes the "Resume Queue" logic *without* prompting to filter by genre. Processes the entire queue, attempts to make tracks available, and then downloads available tracks. Designed for automated execution.

## `dj-wizard url`

Manages a persistent list of Soundeo URLs (e.g., labels, charts you follow).

* Presents an interactive menu with the following options:
    * **Add To Url List:**
        * Prompts for a Soundeo URL.
        * Adds the URL to the `url_list` in `soundeo_log.json` if not already present.
    * **Download From Url:**
        * Prompts for a Soundeo URL.
        * Scrapes track IDs from the URL.
        * Attempts to download *all* tracks found directly. Asks about re-downloading previously downloaded tracks during the process.

## `dj-wizard clean`

Scans a selected directory for duplicate audio files (based on content hash) and empty subfolders, then removes them.

* Prompts the user to select a starting directory using a system dialog.
* Recursively scans the selected directory.
* Prints information about duplicates found/removed and empty folders deleted.

## `dj-wizard info`

Fetches and displays detailed information about a specific Soundeo track.

* Prompts for a Soundeo Track ID.
* Calls the Soundeo API to get track metadata (Title, Artist, Label, Genre, BPM, Key, etc.).
* Prints the retrieved information.

## `dj-wizard spotify`

Integrates with Spotify playlists to find and download corresponding tracks from Soundeo.

### Spotify API Test Configuration

**Note:** This configuration is currently only required to run the specific unit tests that interact directly with the Spotify API (`test_get_playlist_from_api`). The main application functionality for fetching playlist data uses web scraping and does not require these credentials.

To run the API integration tests, you need to set up your Spotify API credentials:

1.  Create a file named `.env` in the project's root directory.
2.  Open the file and add the following lines:

    ```
    SPOTIFY_CLIENT_ID=your_client_id
    SPOTIFY_CLIENT_SECRET=your_client_secret
    ```

3.  Replace `your_client_id` and `your_client_secret` with your actual Spotify API credentials.
4.  You can get these credentials from your Spotify Developer Dashboard.

* Presents an interactive menu with the following options:
    * **Add New Playlist:**
        * Prompts for a public Spotify playlist URL.
        * Scrapes the playlist name and track list (Title, Artist, Spotify ID) using `headless_chrome`.
        * Saves the playlist information to `soundeo_log.json`.
    * **Update Playlist:**
        * Lists playlists previously added (found in the log).
        * Prompts the user to select a playlist by name.
        * Re-scrapes the selected playlist's information from Spotify.
        * Updates the playlist data in `soundeo_log.json`.
    * **Sync Public Playlists:**
        * Fetches all public playlists from the logged-in user's Spotify account.
        * For each public playlist, fetches the full track list.
        * Adds any new public playlists to the log, and updates any existing ones.
    * **Download Tracks From Playlist:**
        * Lists previously added playlists.
        * Prompts the user to select a playlist by name.
        * For each track in the selected Spotify playlist:
            * Checks for existing Soundeo mapping in the log.
            * If no mapping exists, searches Soundeo using Artist/Title.
            * If multiple Soundeo tracks match, prompts the user to select the correct one (or skip).
            * Stores the mapping (Spotify ID -> Soundeo ID or None) in the log.
            * Attempts to download the mapped Soundeo tracks using the standard download logic.
    * **Print Downloaded Tracks By Playlist:**
        * Lists previously added playlists.
        * Prompts the user to select a playlist by name.
        * Checks the log for tracks in that playlist that have been successfully mapped to a Soundeo ID *and* marked as downloaded.
        * Prints a list of these downloaded tracks with their Soundeo URL.
    * **Create Spotify Playlist File:** (Note: Implementation seems incomplete in provided source)
        * Lists previously added playlists.
        * Prompts the user to select a playlist.
        * Intended to create an M3U8 (or similar) playlist file based on the downloaded tracks for that Spotify playlist.
    * **Download From All Playlists:**
        * Scans all locally saved Spotify playlists.
        * For every unpaired track, it attempts to find a single, unambiguous match on Soundeo.
        * Tracks with a single match are automatically paired and added to the download queue with High priority.
        * Tracks with no matches or multiple matches are skipped.
        * After processing all playlists, it automatically starts the download queue.
    * **Organize Downloads By Playlist:**
        * Scans the main download directory.
        * Prompts the user to select which playlists to organize (all are selected by default).
        * For each selected playlist, creates a subfolder.
        * Copies any locally found tracks belonging to that playlist into its respective folder.
        * If a track is missing locally but was previously downloaded (e.g., deleted manually), it is automatically re-downloaded directly, bypassing the queue.
        * If tracks are missing and were never downloaded, and this happens for more than one playlist, it will print a report asking the user to pair them manually.