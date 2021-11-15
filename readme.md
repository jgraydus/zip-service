# zip-service

This is an HTTP service which downloads files and streams them back to you as a single zip file.

The request body looks like this:

```javascript
[
  {
    "filename": "foo.jpg",
    "url": "https://www.example.com/blahblahblah.jpg"
  },
  {
    "filename": "bar.jpg",
    "url": "https://s3.amazon.com/thisisanexample/blah.jpg"
  },
  // etc...
]
```
The response is a zip file containing all the indicated files.

## Instructions
- install the Rust tool chain (I recommend rustup: https://rustup.rs/)
- (if compiling on macOS) ensure you have the command line build tools: `xcode-select install`
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
My primary goal was to keep the memory per request bounded no matter how many files the user 
asks for or the size of the files. To accomplish this, files are retrieved one at a time. As the file data 
arrives, it is immediately compressed and sent to the response stream.

## TODO
- write tests
- add better error handling
- reduce allocations
- implement more of the zip spec, in particular zip64 extensions and different compression options
