# ByteBeam

An extremely simple way to stream a file from one machine to another using a self-hosted proxy.

**This is in no way ready for deployment!**

## Background

I often using Magic Wormhole but it keeps selecting the proxies in order to send data, which causes major speed losses and general frusteration, so I decided to make a solution that works well for me.

My requirements were pretty simple: Use an HTTP server to stream a file from one client to another, using an intermediary HTTP server to handle the proxying. The upload and download should work entirely using multipart form streaming, being compatible with CURL for both the upload and download portions. I don't want to force people to download my tool, so for uploads I would be fine requiring a client for advanced features, but downloads need to be done using regular HTTP download methods for at least basic streaming usage.

I don't want the scope of this to extend to a whole file-hosting system with a bunch of users, folders, and a bunch of local disk usage, it's simply meant to be a way to get files from Person A to Person B, where Person A's server does all the work.

The code here was made from multiple iterations of me trying to learn, so it won't be the cleanest, constructive feedback is always helpful! Since this is mostly a learning project for the time being, PRs might not be considered unless I requested help somewhere.

## Server Usage
The server take environment variables to run, currently just being `AUTH` and `LISTEN` where:

- `AUTH`: A secret key used to authenticate the proxying.
- `LISTEN`: The address:port on which the HTTP server should listen. By default is 0.0.0.0:3000

I would highly recommend putting this behing some sort of nginx reverse proxy with SSL. This does not handle encryption at all.

From here it is as simple as `cargo run --release --bin server`

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
          - PROXIED_SERVER=https://proxied-server-address
```

## Client Usage
Client usage can be as simple as doing `curl --form file='@filename' LISTEN/filename`, where the transfer will be started.

The name of the actual file has nothing to do with the upload path, but when downloading the upload path will be the name used.

The client here should be a little more feature-full, soon supporting resumes and some other options such as reverse-uploading.

The required environment variables are:

- `AUTH`: The same secret key used to authenticate the proxying.
- `SERVER`: The address to the server, including http/s.
- `PROXIED_SERVER`: The address of the server that will be proxied through.

`PROXIED_SERVER` is not connected to at all by the client, but just allows it to give you the upload url.

Often times `PROXIED_SERVER` can be the same as `SERVER`, however in my use case I connect directly to the server over WireGuard, so I connect using a different more direct path.

It's usage is as simple as `cargo run --release --bin client -- filename`.

From here it should give a progress bar of it uploading and then stopping around 1GB of upload. The server proxies an estimated GB of data so downloads can be sped up near the start, and if the file is under 1GB, entirely held on the server.

This buffer does sit in RAM, so multiple files could cause some memory management issues. This limit will be editable in the future.

## Downloading
The URL you upload to is the URL you download from currently. Uploading is a POST and downloading is a GET. The download can only occur once and the moment the download begins the file is locked to that specific request, so if it fails the transfer needs to be completely re-done.

## TODOs:
*The content nested is somewhat the thoughts I'm having for solution*

- [x] Upload should be its own little rust program on my side so a link can be auto generated for the content
- [x] Server side caching if requested
- [ ] Streaming a folder through tar.gz/zip
- [ ] Streaming input
- [ ] Client start/better progress
    - currently just uploads and holds, should give some server-side updates
    - Perhaps could be done with some other HTTP requests that give status but are not required
- [ ] Restart on fail as an option
    - Redirect client to a unique url which claims the MPSC?
- [ ] Handle upload/download dying quickly
    - client currently seems to infinitely upload if the reader dies
    - prelimiary resume works, but seems to lose some data, may need to be client implementation
- [x] Download link should give landing page instead of immediate download
- [ ] Get file size from request instead of a form option
- [ ] Add some query args on upload to add some requirements/uptions
- [ ] Allow for reverse uploading/creation of single-use upload keys
    - Reverse process where the listener is consumed first by the initiator
    - Allow for a simple upload interface
- [ ] Better front end
    - Management of what's currently active, memory usage, etc
- [ ] A concept of multi-user instead of single auth key
- [ ] Smarter cache instead of assuming 4096 byte chunks
    - also adding management of cache size
- [ ] Expiry of uploads that are entirely within cache after a given time
- [ ] Better logging for client and server, a little less "de-buggy"
- [ ] Possibly go fully into wormhole territory and encrypt client side
    - Possibly do it with a pre-shared secret since things tend to be one-direction
        - is there an easy way to do key exchange without both people needing the client?
    - Allow for decryption using built-in tools when downloading using openssl and curl
- [ ] Hold client state in some config instead of needing envionment variables for all usage