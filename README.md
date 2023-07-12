# DJ wizard

Automatically download all the songs from a Soundeo.com url to the given path.

It also stores a record of the already downloaded files.

## Setup

Run:

```
dj-wizard login
```

Fill the necessary data and select the folder to download the tracks. 

The `config.json` file is stored at `$HOME/.soundeo_bot_config`: 

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

Select _Add to queue_ to queue them, and then select _Resume queue_ to download them. 

## Url

You can start downloading directly from a url by running

```shell
dj-wizard url
```

## Hint

To use this program on multiple computers, share the `config.json` file on a shared folder as Google Drive or Dropbox.


