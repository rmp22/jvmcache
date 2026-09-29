use crate::domain::JvmCacheError;
use std::collections::HashMap;

const MAGIC_CAFEBABE: u32 = 0xCAFEBABE;
const ACC_PRIVATE: u16 = 0x0002;
const ACC_STATIC: u16 = 0x0008;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassMember {
    pub name: String,
    pub descriptor: String,
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedClass {
    pub this_class: String,
    pub super_class: Option<String>,
    pub interfaces: Vec<String>,
    pub non_private_fields: Vec<ClassMember>,
    pub non_private_methods: Vec<ClassMember>,
}

pub struct BytecodeParser<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> BytecodeParser<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<ParsedClass, JvmCacheError> {
        let mut parser = Self { bytes, cursor: 0 };
        parser.parse_internal()
    }

    fn parse_internal(&mut self) -> Result<ParsedClass, JvmCacheError> {
        let magic = self.read_u32()?;
        if magic != MAGIC_CAFEBABE {
            return Err(JvmCacheError::Execution("Invalid class file magic".to_string()));
        }

        let _minor = self.read_u16()?;
        let _major = self.read_u16()?;
        let cp_count = self.read_u16()? as usize;

        let mut utf8_map: HashMap<usize, String> = HashMap::with_capacity(cp_count);
        let mut class_map: HashMap<usize, usize> = HashMap::with_capacity(cp_count / 4);

        let mut idx = 1;
        while idx < cp_count {
            let tag = self.read_u8()?;
            match tag {
                1 => {
                    let len = self.read_u16()? as usize;
                    let slice = self.read_slice(len)?;
                    let s = String::from_utf8_lossy(slice).to_string();
                    utf8_map.insert(idx, s);
                    idx += 1;
                }
                7 | 16 | 19 | 20 => {
                    let name_idx = self.read_u16()? as usize;
                    if tag == 7 {
                        class_map.insert(idx, name_idx);
                    }
                    idx += 1;
                }
                8 => {
                    self.skip(2)?;
                    idx += 1;
                }
                3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => {
                    self.skip(4)?;
                    idx += 1;
                }
                5 | 6 => {
                    self.skip(8)?;
                    idx += 2;
                }
                15 => {
                    self.skip(3)?;
                    idx += 1;
                }
                _ => return Err(JvmCacheError::Execution(format!("Unknown CP tag {tag}"))),
            }
        }

        let _access_flags = self.read_u16()?;
        let this_class_idx = self.read_u16()? as usize;
        let super_class_idx = self.read_u16()? as usize;

        let resolve_class_name = |c_idx: usize| -> Option<String> {
            let name_idx = *class_map.get(&c_idx)?;
            utf8_map.get(&name_idx).cloned()
        };

        let this_class = resolve_class_name(this_class_idx)
            .ok_or_else(|| JvmCacheError::Execution("Missing this_class in CP".to_string()))?;
        let super_class = resolve_class_name(super_class_idx);

        let interfaces_count = self.read_u16()? as usize;
        let mut interfaces = Vec::with_capacity(interfaces_count);
        for _ in 0..interfaces_count {
            let iface_idx = self.read_u16()? as usize;
            if let Some(name) = resolve_class_name(iface_idx) {
                interfaces.push(name);
            }
        }

        let non_private_fields = self.read_members(&utf8_map)?;
        let non_private_methods = self.read_members(&utf8_map)?;

        Ok(ParsedClass {
            this_class,
            super_class,
            interfaces,
            non_private_fields,
            non_private_methods,
        })
    }

    fn read_members(&mut self, utf8_map: &HashMap<usize, String>) -> Result<Vec<ClassMember>, JvmCacheError> {
        let count = self.read_u16()? as usize;
        let mut members = Vec::with_capacity(count);

        for _ in 0..count {
            let flags = self.read_u16()?;
            let name_idx = self.read_u16()? as usize;
            let desc_idx = self.read_u16()? as usize;

            let is_non_private = (flags & ACC_PRIVATE) == 0;
            let is_static = (flags & ACC_STATIC) != 0;

            if is_non_private {
                if let (Some(name), Some(descriptor)) = (utf8_map.get(&name_idx), utf8_map.get(&desc_idx)) {
                    members.push(ClassMember {
                        name: name.clone(),
                        descriptor: descriptor.clone(),
                        is_static,
                    });
                }
            }

            let attr_count = self.read_u16()? as usize;
            for _ in 0..attr_count {
                self.skip(2)?;
                let len = self.read_u32()? as usize;
                self.skip(len)?;
            }
        }

        Ok(members)
    }

    fn read_u8(&mut self) -> Result<u8, JvmCacheError> {
        if self.cursor >= self.bytes.len() {
            return Err(JvmCacheError::Execution("Unexpected EOF reading u8".to_string()));
        }
        let b = self.bytes[self.cursor];
        self.cursor += 1;
        Ok(b)
    }

    fn read_u16(&mut self) -> Result<u16, JvmCacheError> {
        let slice = self.read_slice(2)?;
        Ok(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, JvmCacheError> {
        let slice = self.read_slice(4)?;
        Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn read_slice(&mut self, len: usize) -> Result<&'a [u8], JvmCacheError> {
        if self.cursor + len > self.bytes.len() {
            return Err(JvmCacheError::Execution("Unexpected EOF reading slice".to_string()));
        }
        let s = &self.bytes[self.cursor..self.cursor + len];
        self.cursor += 1 * len;
        Ok(s)
    }

    fn skip(&mut self, len: usize) -> Result<(), JvmCacheError> {
        if self.cursor + len > self.bytes.len() {
            return Err(JvmCacheError::Execution("Unexpected EOF during skip".to_string()));
        }
        self.cursor += len;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_real_class_file() {
        let mut dummy = Vec::new();
        dummy.extend_from_slice(&MAGIC_CAFEBABE.to_be_bytes());
        dummy.extend_from_slice(&0u16.to_be_bytes());
        dummy.extend_from_slice(&65u16.to_be_bytes());
        dummy.extend_from_slice(&5u16.to_be_bytes());
        dummy.push(1);
        dummy.extend_from_slice(&10u16.to_be_bytes());
        dummy.extend_from_slice(b"com/FooBar");
        dummy.push(7);
        dummy.extend_from_slice(&1u16.to_be_bytes());
        dummy.push(1);
        dummy.extend_from_slice(&16u16.to_be_bytes());
        dummy.extend_from_slice(b"java/lang/Object");
        dummy.push(7);
        dummy.extend_from_slice(&3u16.to_be_bytes());
        dummy.extend_from_slice(&0x0001u16.to_be_bytes());
        dummy.extend_from_slice(&2u16.to_be_bytes());
        dummy.extend_from_slice(&4u16.to_be_bytes());
        dummy.extend_from_slice(&0u16.to_be_bytes());
        dummy.extend_from_slice(&0u16.to_be_bytes());
        dummy.extend_from_slice(&0u16.to_be_bytes());
        dummy.extend_from_slice(&0u16.to_be_bytes());

        let parsed = BytecodeParser::parse(&dummy).unwrap();
        assert_eq!(parsed.this_class, "com/FooBar");
        assert_eq!(parsed.super_class, Some("java/lang/Object".to_string()));
        assert!(parsed.interfaces.is_empty());
    }
}
