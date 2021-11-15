use podio::{LittleEndian, WritePodExt};
use crc32fast::Hasher;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;
use hyper::body::{Sender, Bytes};

struct FileMetadata {
    crc32: u32,
    uncompressed_size: u32,
    compressed_size: u32,
    offset: u32,
    file_name: String,
}

struct CurrentFileState {
    file_metadata: FileMetadata,
    hasher: Hasher,
    encoder: DeflateEncoder<Vec<u8>>,
}

/**
write a multifile zip file to the given Sender.

for each file:
- call start_file once
- then call write for each chunk of the file data
- then call finish_file once

files must be written sequentially (ie don't interleave calls to the above functions)

when all files are done:
- call finish

TODO implement zip64 extensions
TODO reduce copying and allocation
 */
pub struct ZipWriter {
    sender: Sender,
    file_metadata: Vec<FileMetadata>,
    bytes_written: u32,
    current_file_state: Option<CurrentFileState>,
}

impl ZipWriter {
    pub fn new(sender: Sender) -> Self {
        Self {
            sender,
            file_metadata: Vec::new(),
            bytes_written: 0,
            current_file_state: None,
        }
    }

    /** prepares state to start writing data for a file and writes the local file header */
    pub async fn start_file(&mut self, file_name: &str) -> Result<(), hyper::Error> {
        if let Some(_) = self.current_file_state {
            panic!("call finish_file before starting a new file");
        }

        let file_metadata = FileMetadata {
            crc32: 0,
            uncompressed_size: 0,
            compressed_size: 0,
            offset: self.bytes_written,
            file_name: file_name.into(),
        };

        // TODO avoid this buffer or buffer without allocation
        let mut buf = Vec::new();
        let header_size = write_local_file_header(&mut buf, &file_metadata).unwrap();
        self.sender.send_data(Bytes::from(buf)).await?;

        self.current_file_state = Some(CurrentFileState {
            file_metadata,
            hasher: Hasher::new(),
            encoder: DeflateEncoder::new(
                Vec::new(),
                Compression::default()
            ),
        });

        self.bytes_written = self.bytes_written + header_size;

        Ok(())
    }

    /** write part or all of file data */
    pub async fn write(&mut self, buf: &[u8]) -> Result<(), hyper::Error> {
        if let Some(CurrentFileState {
                        file_metadata,
                        hasher,
                        encoder,
                 }) = &mut self.current_file_state {

            file_metadata.uncompressed_size = file_metadata.uncompressed_size + buf.len() as u32;

            // update the checksum
            hasher.update(buf);

            // compress the given chunk of data and write the new blocks to the response
            encoder.write_all(buf).unwrap();

            // swap out the encoder's buffer
            let encoder_buf = std::mem::take(encoder.get_mut());
            file_metadata.compressed_size = file_metadata.compressed_size + encoder_buf.len() as u32;

            // send the compressed data
            self.sender.send_data(Bytes::from(encoder_buf)).await?;

            return Ok(())
        }

        panic!("cannot write until start_file is called")
    }

    /** complete the writing of a file */
    pub async fn finish_file(&mut self) -> Result<(), hyper::Error> {
        let current_file_state = std::mem::take(&mut self.current_file_state).unwrap();
        let mut file_metadata = current_file_state.file_metadata;

        // finished checksum
        file_metadata.crc32 = current_file_state.hasher.finalize();

        // finalize the encoder. this flushes the encoder's internal buffer and so might return
        // some data that hasn't been written to the response yet
        let remaining_data = current_file_state.encoder.finish().unwrap();
        file_metadata.compressed_size = file_metadata.compressed_size + remaining_data.len() as u32;
        self.sender.send_data(Bytes::from(remaining_data)).await?;

        // TODO avoid this buffer or buffer without allocation
        let mut buf = Vec::new();
        let data_descriptor_size = write_data_descriptor(&mut buf, &file_metadata).unwrap();
        self.sender.send_data(Bytes::from(buf)).await?;

        self.bytes_written = self.bytes_written + file_metadata.compressed_size + data_descriptor_size;
        self.file_metadata.push(file_metadata);

        Ok(())
    }

    /** complete the zip file by writing out the central directory. consumes self */
    pub async fn finish(mut self) -> Result<(), hyper::Error> {
        let offset = self.bytes_written;

        for file in self.file_metadata.iter() {
            // TODO avoid this buffer or buffer without allocation
            let mut buf = Vec::new();
            let bytes_written = write_central_directory_header(&mut buf, file).unwrap();
            self.sender.send_data(Bytes::from(buf)).await?;
            self.bytes_written = self.bytes_written + bytes_written;
        }
        let size = self.bytes_written - offset;

        // TODO avoid this buffer or buffer without allocation
        let mut buf = Vec::new();
        write_end_of_central_directory_record(
            &mut buf,
            self.file_metadata.len() as u16,
            offset,
            size,
        ).unwrap();
        self.sender.send_data(Bytes::from(buf)).await?;

        Ok(())
    }
}

/*
   4.3.7  Local file header:

      local file header signature     4 bytes  (0x04034b50)
      version needed to extract       2 bytes
      general purpose bit flag        2 bytes
      compression method              2 bytes
      last mod file time              2 bytes
      last mod file date              2 bytes
      crc-32                          4 bytes
      compressed size                 4 bytes
      uncompressed size               4 bytes
      file name length                2 bytes
      extra field length              2 bytes

      file name (variable size)
      extra field (variable size)
 */
fn write_local_file_header<W: std::io::Write>(
    writer: &mut W,
    file: &FileMetadata,
) -> std::io::Result<u32> {
    // local file header signature
    writer.write_u32::<LittleEndian>(0x04034b50)?;

    // version
    writer.write_u16::<LittleEndian>(0x0014)?;

    // flags
    writer.write_u16::<LittleEndian>(1 << 3)?; // bit 3 indicates data descriptors in use

    // compression method
    writer.write_u16::<LittleEndian>(8)?; // 8 = deflate

    // last mod file time
    writer.write_u16::<LittleEndian>(0)?; // TODO

    // last mod file date
    writer.write_u16::<LittleEndian>(0)?; // TODO

    // crc-32
    writer.write_u32::<LittleEndian>(0)?;

    // compressed size
    writer.write_u32::<LittleEndian>(0)?;

    // uncompressed size
    writer.write_u32::<LittleEndian>(0)?;

    // file name length
    let file_name = file.file_name.as_bytes();
    writer.write_u16::<LittleEndian>(file_name.len() as u16)?;

    // extra field length
    writer.write_u16::<LittleEndian>(0)?;

    writer.write_all(file_name)?;

    // extra field TODO

    Ok(30 + file_name.len() as u32)
}

/*
   4.3.9  Data descriptor:

        signature                       4 bytes (0x08074b50)
        crc-32                          4 bytes
        compressed size                 4 bytes
        uncompressed size               4 bytes
 */
fn write_data_descriptor<W: std::io::Write>(
    writer: &mut W,
    file: &FileMetadata,
) -> std::io::Result<u32> {
    // local file header signature
    writer.write_u32::<LittleEndian>(0x08074b50)?;

    // crc-32
    writer.write_u32::<LittleEndian>(file.crc32)?;

    // compressed size
    writer.write_u32::<LittleEndian>(file.compressed_size)?;

    // uncompressed size
    writer.write_u32::<LittleEndian>(file.uncompressed_size)?;

    Ok(16)
}

/*

   4.3.12  Central directory structure:

      [central directory header 1]
      .
      .
      .
      [central directory header n]
      [digital signature]

      File header:

        central file header signature   4 bytes  (0x02014b50)
        version made by                 2 bytes
        version needed to extract       2 bytes
        general purpose bit flag        2 bytes
        compression method              2 bytes
        last mod file time              2 bytes
        last mod file date              2 bytes
        crc-32                          4 bytes
        compressed size                 4 bytes
        uncompressed size               4 bytes
        file name length                2 bytes
        extra field length              2 bytes
        file comment length             2 bytes
        disk number start               2 bytes
        internal file attributes        2 bytes
        external file attributes        4 bytes
        relative offset of local header 4 bytes

        file name (variable size)
        extra field (variable size)
        file comment (variable size)
 */
fn write_central_directory_header<W: std::io::Write>(
    writer: &mut W,
    file: &FileMetadata,
) -> std::io::Result<u32> {
    // signature
    writer.write_u32::<LittleEndian>(0x02014b50)?;

    // version made by
    writer.write_u16::<LittleEndian>((3u16 << 8) | 46u16)?; // TODO explain

    // version needed to extract
    writer.write_u16::<LittleEndian>(0x0014)?;

    // flags
    writer.write_u16::<LittleEndian>(1 << 3)?; // bit 3 indicates data descriptors in use

    // compression method
    writer.write_u16::<LittleEndian>(8)?; // 8 = deflate

    // last mod file time
    writer.write_u16::<LittleEndian>(0)?; // TODO

    // last mod file date
    writer.write_u16::<LittleEndian>(0)?; // TODO

    // crc-32
    writer.write_u32::<LittleEndian>(file.crc32)?;

    // compressed size
    writer.write_u32::<LittleEndian>(file.compressed_size)?;

    // uncompressed size
    writer.write_u32::<LittleEndian>(file.uncompressed_size)?;

    // file name length
    let file_name = file.file_name.as_bytes();
    writer.write_u16::<LittleEndian>(file_name.len() as u16)?;

    // extra field length
    writer.write_u16::<LittleEndian>(0)?;

    // file comment length
    writer.write_u16::<LittleEndian>(0)?;

    // disk number start
    writer.write_u16::<LittleEndian>(0)?;

    // internal file attributes
    writer.write_u16::<LittleEndian>(0)?; // TODO

    // external file attributes
    writer.write_u32::<LittleEndian>(0o100644 << 16)?; // TODO explain

    // relative offset of local header
    writer.write_u32::<LittleEndian>(file.offset)?;

    // file name
    writer.write_all(file_name)?;

    // extra field (variable size) // TODO
    // file comment (variable size) // TODO

    Ok(46 + file_name.len() as u32)
}

/*
   4.3.16  End of central directory record:

      end of central dir signature    4 bytes  (0x06054b50)
      number of this disk             2 bytes
      number of the disk with the
      start of the central directory  2 bytes
      total number of entries in the
      central directory on this disk  2 bytes
      total number of entries in
      the central directory           2 bytes
      size of the central directory   4 bytes
      offset of start of central
      directory with respect to
      the starting disk number        4 bytes
      .ZIP file comment length        2 bytes
      .ZIP file comment       (variable size)
 */
fn write_end_of_central_directory_record<W: std::io::Write>(
    writer: &mut W,
    number_of_entries: u16,
    offset: u32,
    size: u32,
) -> std::io::Result<()> {
    // signature
    writer.write_u32::<LittleEndian>(0x06054b50)?;

    // number of this disk
    writer.write_u16::<LittleEndian>(0)?;

    // number of the disk with the start of the central directory
    writer.write_u16::<LittleEndian>(0)?;

    // total number of entries in the central directory on this disk
    writer.write_u16::<LittleEndian>(number_of_entries)?;

    // total number of entries in the central directory
    writer.write_u16::<LittleEndian>(number_of_entries)?;

    // size of the central directory
    writer.write_u32::<LittleEndian>(size)?;

    // offset of start of central directory with respect to the starting disk number
    writer.write_u32::<LittleEndian>(offset)?;

    // .ZIP file comment length
    writer.write_u16::<LittleEndian>(0)?;

    // .ZIP file comment TODO

    Ok(())
}
