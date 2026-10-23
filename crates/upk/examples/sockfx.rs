//! Dev: socket effects (References: StartTag / EndTag / Reference / ValidView / EndTime) of a
//! skeletal mesh by object name.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, _) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e).eq_ignore_ascii_case(&a[2]) && p.export_class(e) == "SkeletalMesh") {
        println!("== #{} {}", i + 1, p.object_path(i as i32 + 1));
        for s in p.mesh_sockets(i) {
            for e in &s.effects {
                println!("{:24} {:14} start {:20} end {:16} {:8} {:5.1} {} [{}]", s.name, s.bone, e.start_tag, e.end_tag, e.view, e.end_time, e.reference, e.class);
            }
        }
    }
}
