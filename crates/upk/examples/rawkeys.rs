//! Dev: raw rotation key words of one track of a sequence (IntervalFixed32NoW).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let tr: usize = a[4].parse().unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "AnimSequence" { continue; }
        let Some(pr) = p.export_props(i) else { continue };
        if pr.enum_name("SequenceName") != Some(a[3].as_str()) || !p.object_path(i as i32 + 1).contains(&a[2]) { continue; }
        let mut r = p.export_reader(i);
        let _ = p.read_props(&mut r);
        let n = r.i32().unwrap();
        let st = r.bytes(n as usize).unwrap().to_vec();
        let offs = match pr.get("CompressedTrackOffsets") { Some(upk::props::Value::Raw { data, .. }) => data[4..].chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>(), _ => vec![] };
        let t = &offs[tr * 4..tr * 4 + 4];
        println!("{} track {tr}: {:?} stream {}", p.object_path(i as i32 + 1), t, st.len());
        let ro = t[2] as usize;
        let f = |o: usize| f32::from_le_bytes(st[o..o + 4].try_into().unwrap());
        println!("  mins {:?} ranges {:?}", [f(ro), f(ro + 4), f(ro + 8)], [f(ro + 12), f(ro + 16), f(ro + 20)]);
        for k in 0..6 { let w = u32::from_le_bytes(st[ro + 24 + k * 4..ro + 28 + k * 4].try_into().unwrap()); println!("  key {k}: {w:08x} x {} y {} z {}", w >> 21, (w >> 10) & 0x7ff, w & 0x3ff); }
        println!("  next track offsets: {:?}", &offs[(tr + 1) * 4..(tr + 2) * 4]);
    }
}
