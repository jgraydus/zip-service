# zip-service

This is an HTTP service which downloads files and streams them back to you as a single zip file.

The request body looks like this:

```json
[
  {
    "filename": "foo.jpg",
    "url": "https://www.example.com/blahblahblah.jpg"
  },
  {
    "filename": "bar.jpg",
    "url": "https://s3.amazon.com/thisisanexample/blah.jpg"
  },
  ...
]
```
The response is a zip file containing all the indicated files.

## Instructions
- install the Rust tool chain (I recommend rustup: https://rustup.rs/)
- execute `cargo run --release` and go get some coffee. when you get back the server should be running
- make a request with your favorite http client. examples:
```bash
curl http://localhost:3000/ \
  -d '[{"url": "https://media.giphy.com/media/3oz8xD0xvAJ5FCk7Di/giphy.gif", "filename": "pic001.gif" }]' \
  -H "Content-Type: application/json" \
  > archive.zip
  
curl http://localhost:3000/ \
  -d "@examples/small_example.json" \
  -H "Content-Type: application/json" \
  > archive.zip
  
curl http://localhost:3000/ \
  -d "@examples/huge_example.json" \
  -H "Content-Type: application/json" \
  > archive.zip
```
## Design notes
Each request may require downloading many files. In order to keep the memory footprint of the server small, the
files are downloaded sequentially and streamed through the zip writer and into the HTTP response.

## TODO
- write tests
- add better error handling
- reduce allocations
- implement more of the zip spec, in particular zip64 extensions and different compression options
