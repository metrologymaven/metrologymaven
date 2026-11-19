p='crates/fid-xtask/src/conic_audit.rs'; s=open(p,encoding='utf-8').read()
old='''fn split_keep_newlines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            out.push(&bytes[start..=i]);
            start = i + 1;
        }
    }
    if start < bytes.len() {
        out.push(&bytes[start..]);
    }
    out
}

'''
assert old in s
s=s.replace(old,'')
s=s.replace('let old_lines = split_keep_newlines(&old_bytes);','let old_lines: Vec<&[u8]> = old_bytes.split_inclusive(|&b| b == b\'\n\').collect();')
s=s.replace("{ t.parse().ok() } else { None }","{ t.parse::<usize>().ok() } else { None }")
open(p,'w',encoding='utf-8',newline='\n').write(s)
