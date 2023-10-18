use std::{mem::{align_of, size_of}, borrow::Cow};

use windows::{Win32::UI::WindowsAndMessaging::{DLGTEMPLATE, DLGITEMTEMPLATE, DS_SETFONT}, core::{PCSTR, PCWSTR}};

pub enum WindowClass<'a> {
    None,
    Atom(u16),
    String(Cow<'a, str>)
}

impl<'a> From<&'a str> for WindowClass<'a> {
    fn from(value: &'a str) -> Self { WindowClass::String(Cow::Borrowed(value)) }
}

impl TryFrom<PCSTR> for WindowClass<'_> {
    type Error = std::string::FromUtf8Error;
    fn try_from(value: PCSTR) -> Result<Self, Self::Error> {
        if value.is_null() {
            Ok(WindowClass::None)
        } else if (value.0 as usize) & 0xff00_usize == 0 {
            Ok(WindowClass::Atom(value.0 as u16))
        }  else {
            Ok(WindowClass::String(Cow::Owned(unsafe { value.to_string()? })))
        }
    }
}

impl TryFrom<PCWSTR> for WindowClass<'_> {
    type Error = std::string::FromUtf16Error;
    fn try_from(value: PCWSTR) -> Result<Self, Self::Error> {
        if value.is_null() {
            Ok(WindowClass::None)
        } else if (value.0 as usize) & 0xff00_usize == 0 {
            Ok(WindowClass::Atom(value.0 as u16))
        }  else {
            Ok(WindowClass::String(Cow::Owned(unsafe { value.to_string()? })))
        }
    }
}

#[allow(unused)]
pub enum DialogControlTitle<'a> {
    None,
    Resource(u16),
    Text(Cow<'a, str>)
}

pub struct DialogControl<'a> {
    pub id: u16,
    pub class: WindowClass<'a>,

    pub style: u32,
    pub ext_style: u32,
    pub pos: (i16, i16),
    pub size: (i16, i16),

    pub title: DialogControlTitle<'a>,
    pub creation_data: Option<&'a [u8]>
}

pub struct DialogFont<'a> {
    pub typeface: Cow<'a, str>,
    pub point_size: u16
}

pub fn build_dialog_template<'a>(buf: &'a mut [u8], class: WindowClass, title: &str, style: u32, ext_style: u32, size: (i16, i16), font: Option<DialogFont>, controls: &[DialogControl]) -> &'a DLGTEMPLATE {
    let mut buf_ptr: *mut u8 = buf.as_mut_ptr();

    macro_rules! append_to_template {
        ($type: ty) => { append_to_template!($type, align_of::<$type>()) };
        ($type: ty, $align: expr) => { { &mut append_to_template!($type, 1, $align)[0] } };
        ($type: ty, $count: expr, $align: expr) => {
            unsafe {
                //Align the buffer pointer
                assert!($align >= align_of::<$type>());
                let align_off = buf_ptr.align_offset($align);
                buf_ptr = buf_ptr.add(align_off);
         
                //Obtain a pointer to the entry
                let entry_ptr = buf_ptr.cast::<$type>();
                buf_ptr = buf_ptr.add(size_of::<$type>() * $count);

                //Ensure we didn't overflow
                if !buf.as_mut_ptr_range().contains(&buf_ptr) {
                    panic!("ran out of buffer space while building Win32 dialog template")
                }

                std::slice::from_raw_parts_mut::<'a>(entry_ptr, $count)
            }
        };
    }
    macro_rules! append_str_to_template {
        ($str: expr) => {
            {
                for chr in $str.encode_utf16() {
                    *append_to_template!(u16) = chr;
                }
                *append_to_template!(u16) = 0_u16;
            }
        };
    }

    //Append the main template struct
    let mut style = style;
    if font.is_some() {
        style |= DS_SETFONT as u32;
    } else if (style & DS_SETFONT as u32) != 0 {
        panic!("DS_SETFONT flag set without specifiying a dialog font");
    }

    let template = append_to_template!(DLGTEMPLATE, 4);
    *template = DLGTEMPLATE{
        style: style,
        dwExtendedStyle: ext_style,
        cdit: u16::try_from(controls.len()).expect("too many dialog template controls"),
        x: 0,
        y: 0,
        cx: size.0,
        cy: size.1
    };

    //Append the menu
    *append_to_template!(u16) = 0_u16; //No menu

    //Append the class
    match class {
        WindowClass::None => *append_to_template!(u16) = 0_u16,
        WindowClass::Atom(class) => {
            *append_to_template!(u16) = 0xffff_u16;
            *append_to_template!(u16) = class;
        }
        WindowClass::String(class) => append_str_to_template!(class)
    }

    //Append the title
    append_str_to_template!(title);

    //Append the font
    if let Some(font) = font {
        *append_to_template!(u16) = font.point_size;
        append_str_to_template!(font.typeface);
    }

    //Append the controls
    for control in controls {
        //Append the item entry
        *append_to_template!(DLGITEMTEMPLATE, 4) = DLGITEMTEMPLATE{
            style: control.style,
            dwExtendedStyle: control.ext_style,
            x: control.pos.0,
            y: control.pos.1,
            cx: control.size.0,
            cy: control.size.1,
            id: control.id
        };

        //Append the control class
        match &control.class {
            WindowClass::None => panic!("dialog controls cannot not have a class"),
            WindowClass::Atom(class) => {
                *append_to_template!(u16) = 0xffff_u16;
                *append_to_template!(u16) = *class;
            }
            WindowClass::String(class) => append_str_to_template!(class)
        }

        //Append the control title
        match &control.title {
            DialogControlTitle::None => *append_to_template!(u16) = 0_u16,
            DialogControlTitle::Resource(res) => {
                *append_to_template!(u16) = 0xffff_u16;
                *append_to_template!(u16) = *res;
            }
            DialogControlTitle::Text(txt) => append_str_to_template!(txt)
        }

        //Append the control creation data
        match control.creation_data {
            Some(data) => {
                *append_to_template!(u16) = u16::try_from(2 + data.len()).expect("dialog control creation data too long");
                append_to_template!(u8, data.len(), 2).copy_from_slice(data);
            }
            None => *append_to_template!(u16) = 0
        }
    }

    template
}