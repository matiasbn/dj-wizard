# DJ wizard

Automatically download songs from a Soundeo.com url.

It also stores a record of the already downloaded files.

## Installation

Go to the terminal to run the next commands (use Spotlight and search for Terminal). 

The copy and paste the following commands

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Install `dj-wizard`:

```bash
cargo install dj-wizard
```

## Setup

Run:

```
dj-wizard login
```

Fill the necessary data and select the folder to download the tracks. 

The `config.json` file is stored at `$HOME/.dj_wizard_config`: 

```json
{
  "user": "",
  "pass": "",
  "download_path": ""
}
```

Run the next command for more info:

```shell
dj-wizard --help
```

You can download songs from any valid soundeo.com page. The program is not validating the correct link so be careful.

## Queue

You can queue multiple songs, and then download them by running

```shell
dj-wizard queue
```

Select _Add to queue_, and then paste a _Soundeo.com_ url.

Then, run the command again and select _Resume queue_ to download them.

The program will ask you if you want to filter by genre.

After that, the program will simulate clicking on the "Download" buttons on the webpage, so they will be saved to your collection before downloading.

You can save to the collection by choosing _Save to available tracks_.

## Spotify 

_dj-wizard_ can add songs to the queue from a Spotify playlist.

Run:

```bash
dj-wizard spotify
```

Select _Add new playlist_ and paste the Spotify playlist's url. The playlist should be publicly accessible.

The program will save the playlist data.

Once added, in case you want to update the data, run _Update playlist_.

To download the songs select the _Download tracks from playlist_ option.