//! Lossless-optimize JPEG files from the command line.
//! usage: cargo run --release --example jopt -- [--baseline] <out_dir> <file.jpg>...
//! Prints CSV: file,orig_bytes,opt_bytes,saving_pct,status
fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let baseline = args.peek().map(|a| a == "--baseline").unwrap_or(false);
    if baseline {
        args.next();
    }
    let out_dir = args
        .next()
        .expect("usage: jopt [--baseline] <out_dir> <files...>");
    std::fs::create_dir_all(&out_dir).unwrap();
    println!("file,orig_bytes,opt_bytes,saving_pct,status");
    for f in args {
        let data = std::fs::read(&f).unwrap();
        let name = std::path::Path::new(&f)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let opt = if baseline {
            smallpeg::optimize_jpeg_baseline(&data)
        } else {
            smallpeg::optimize_jpeg(&data)
        };
        match opt {
            Some(o) => {
                let dst = format!("{out_dir}/{name}.opt.jpg");
                std::fs::write(&dst, &o).unwrap();
                let pct = 100.0 * (1.0 - o.len() as f64 / data.len() as f64);
                println!("{f},{},{},{pct:.2},ok", data.len(), o.len());
            }
            None => println!("{f},{},,,unsupported", data.len()),
        }
    }
}
