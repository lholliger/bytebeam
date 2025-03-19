# ByteBeam

An extremely simple way to stream a file from one machine to another using a self-hosted proxy.

**This is in no way ready for deployment!**

Note: This readme is not updated for 0.3.0 yet.

## Background

I often using Magic Wormhole but it keeps selecting the proxies in order to send data, which causes major speed losses and general frusteration, so I decided to make a solution that works well for me.

My requirements were pretty simple: Use an HTTP server to stream a file from one client to another, using an intermediary HTTP server to handle the proxying. The upload and download should work entirely using multipart form streaming, being compatible with CURL for both the upload and download portions. I don't want to force people to download my tool, so for uploads I would be fine requiring a client for advanced features, but downloads need to be done using regular HTTP download methods for at least basic streaming usage.

I don't want the scope of this to extend to a whole file-hosting system with a bunch of users, folders, and a bunch of local disk usage, it's simply meant to be a way to get files from Person A to Person B, where Person A's server does all the work.

The code here was made from multiple iterations of me trying to learn, so it won't be the cleanest, constructive feedback is always helpful! Since this is mostly a learning project for the time being, PRs might not be considered unless I requested help somewhere.

## Installation
This is not currently on Docker or Cargo, so you need to clone the repository in order to install.

After cloning, simply run `cargo install --path .` to install.

If you also want the server to be built, you need to first obtain a wordlist for the files.
Download to `wordlist.txt` with newlines for each word. I use the wordle wordlist since its long and contains simple short words.

From here, run `cargo install --features server --path .`.

## Server Usage
The server take environment variables to run, currently just being `AUTH`, `LISTEN`, and `CACHE` where:

- `AUTH`: A secret key used to authenticate the proxying.
- `LISTEN`: The address:port on which the HTTP server should listen. By default is 0.0.0.0:3000
- `CACHE`: The size in bytes of the cache to use for storing each file. Defaults to 1GB

I would highly recommend putting this behing some sort of nginx reverse proxy with SSL. This does not handle encryption at all. Nginx keepalive limits as well as buffering need to be disabled.

If you want to run this container in docker, just build it `docker build -t bytebeam .` and then run. I run it in docker-compose as follows:
```yml
    bytebeam:
        image: bytebeam
        ports:
          - "3035:3035"
        restart: unless-stopped
        environment:
          - AUTH=password
          - LISTEN=0.0.0.0:3035
```

## Client Usage
Uploading and downloading can all be done using curl, however one side should use ByteBeam (the system has a keepalive timeout which the client handles on its own, as well as handing progress)

To start, it is good to make the config, which is by default read from `~/.config/bytebeam.toml`.

An example for it is as follows:
```toml
auth = "password"

[client]
server = "https://server"
```

These two values are all that are needed at first. They can also be defined using ENV variables. More info is found using `beam up --help`.

From here, you are given a few options. You can either:
1. upload a file
2. download a file
3. download a file from an external upload

## Uploading
When using the client, it is as simple as `beam up [filename]`. It will return a scannable QR code and a URL that will direct a user to the download. The download when opened in a browser will drop you to an interface, while when using wget or curl (really anything that doesnt give `Mozilla` in the user agent) it will download automatically.

The client will have a keepalive signal going until the download is complete, so don't cancel until the other user has completed the download.

## Downloading
Downloading is meant to be as simple as possible, so downloading can be done from the link given by `beam up`, or by doing `wget` to the same path. When using the Beam client, users can simply do `beam down [url]`, and if two users are on the same server, `beam down [number-word-word-word]`.

## Reverse Upload
The client gives you the ability to download from an external upload, which can be done by doing `beam down -o filename`, where filename is where you want to save. From here, it will give a url and qr code with format `[server]/[token]/[key]`. A user can beam up to this using `beam up filename -t [url]`. When using `curl`, they can simply do `curl -F "file=@filename [url]`

This path will be "locked" to the client doing `beam down`, so no one else can take over the download. The upload will cancel if the client doing `down` cancels.

## Curl operation
This system works on a simple enough 4 request system, where there is effectively a `create`, `upload`, `download`, and a sort of keep-alive.

### Create
This is where you create a new upload, and it will return a token that can be used for the actual upload. When using curl this is simply `curl -d "authentication=[password]" https://[server]/[filename]`. This will return JSON as follows:
```json
{
    "file_name":"[filename]",
    "file_size":0,
    "path":"00-words-words-words",
    "upload_key":"01-some-more-words",
    "upload":"NotStarted",
    "download":"NotStarted",
    "created":"2025-03-04T21:37:22.194107721Z",
    "accessed":"2025-03-04T21:37:22.194108495Z"
}
```
Here, the upload and download paths can be inferred.

### Upload
This is where you actually upload a file. Here it is simply `curl -F "file=@[file]" https://[server]/[path]/[upload_key]`. where `path` and `upload_key` were defined in the create request.

The `file` does not need to have the same name as defined in filename. The upload operation does not change the upload name.

### Download
This is much more simple, where it is as simple as `curl https://[server]/[path]`. The server will redirect to the filename specified (`https://[server]/[path]/[filename]`). From here the upload will be piped to this download. Cancelling the request or doing multi-request will result in failure and the need to restart.

### Keep Alive
The system doesn't want to keep cached data any longer than it needs to, so when an upload/download is in progress, a keepalive signal is needed at some point below the cull time defined on the server. The client reuqests this every 10 or so seconds so it can also give up-to-date information. This keepalive is as simple as `curl https://[server]/[path]?status=true`. This will not cause an upload or download, but will update the `accessed` time and return the JSON similar to the create request, however certain values such as the key will be excluded.

## Web Interface
When doing `beam down -o filename`, the page given is web-accessible allow for an upload. It is simply the same link given for the upload path. The reason this interface works is that uploads to `https://[server]/[path]/[key]` for `POST` upload data, while `GET` would normally be for download, but when doing `GET` and the `file` is the same as the `key`, it will return an interface to upload a file.

## TODOs:
*The content nested is somewhat the thoughts I'm having for solution*

- [x] Upload should be its own little rust program on my side so a link can be auto generated for the content
- [x] Server side caching if requested
- [ ] Streaming a folder through tar.gz/zip
- [x] Streaming input
- [x] Client start/better progress
    - Updates done during the keepalive, better progress could be given however
- [ ] Restart on fail as an option
    - System needs to somehow put the mpsc back. Using broadcast leads to issues starting
- [ ] Handle upload/download dying quickly
    - client currently seems to infinitely upload if the reader dies
    - prelimiary resume works, but seems to lose some data, may need to be client implementation
    - under current mpsc the connection seems to just fail
- [x] Download link should give landing page instead of immediate download
- [ ] Get file size from request instead of a form option
- [ ] Add some query args on upload to add some requirements/options
- [x] Allow for reverse uploading/creation of single-use upload keys
    - still needs client implementation
- [ ] Better front end
    - Management of what's currently active, memory usage, etc
- [ ] A concept of multi-user instead of single auth key
    - using SSH key signing
    - allow optional unauthenticated user usage with rate limits
- [ ] Feature for github SSH keys
- [x] Smarter cache instead of assuming 4096 byte chunks
    - also adding management of cache size
- [x] Expiry of uploads that are entirely within cache after a given time
- [x] Better logging for client and server, a little less "de-buggy"
    - server is fairly verbose, client default is good.
- [ ] Possibly go fully into wormhole territory and encrypt client side
    - Possibly do it with a pre-shared secret since things tend to be one-direction
        - is there an easy way to do key exchange without both people needing the client?
    - Allow for decryption using built-in tools when downloading using openssl and curl
- [x] Hold client state in some config instead of needing envionment variables for all usage
    - also perhaps add a "beam config"
    - needs cleanup
- [x] Move server as a feature to remove unneeded features for those only using the client
- [ ] Possibly do away with the "secret" so that one value is for upload and another is for download
    - could be confusing if there is more than one "token" per upload/download pair
- [ ] Reduce CPU usage/increase speed
- [x] HTML upload page, CURL instructions
- [x] Give feedback after upload
    - Give status codes and more details rather than just a string
- [x] Keepalive when doing reverse upload