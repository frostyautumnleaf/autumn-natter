//! A separate XInput pointer for desktop actions. XTEST uses the client pointer of this
//! connection, leaving the pointer of other clients and the physical mouse alone.

use std::sync::{Mutex, OnceLock};
use x11rb::connection::Connection;
use x11rb::protocol::{
    render::{self, ConnectionExt as _},
    xinput::{self, ConnectionExt as _},
    xproto::{self, ConnectionExt as _},
    xtest::ConnectionExt as _,
};
use x11rb::rust_connection::RustConnection;

static POINTER: OnceLock<Mutex<Option<AgentPointer>>> = OnceLock::new();

type Error = Box<dyn std::error::Error>;

pub fn with_pointer<T>(work: impl FnOnce(&AgentPointer) -> Result<T, Error>) -> Result<T, String> {
    let mut state = POINTER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| String::from("The agent pointer is busy."))?;
    if state.is_none() {
        *state = Some(
            AgentPointer::start()
                .map_err(|err| format!("Could not create a separate agent pointer: {err}"))?,
        );
    }
    work(state.as_ref().unwrap()).map_err(|err| err.to_string())
}

pub fn close() {
    if let Some(state) = POINTER.get() {
        if let Ok(mut pointer) = state.lock() {
            *pointer = None;
        }
    }
}

pub struct AgentPointer {
    connection: RustConnection,
    root: xproto::Window,
    device: u16,
    cursor: xproto::Cursor,
}

impl AgentPointer {
    fn start() -> Result<Self, Error> {
        let (connection, screen) = x11rb::connect(None)?;
        let root = connection.setup().roots[screen].root;
        let version = connection.xinput_xi_query_version(2, 0)?.reply()?;
        if version.major_version < 2 {
            return Err("XInput 2 is unavailable".into());
        }
        connection.xtest_get_version(2, 2)?.reply()?;
        let cursor = make_cursor(&connection, root)?;
        let name = format!("Autumn Natter Agent {}", std::process::id());
        let master_name = format!("{name} pointer");
        connection
            .xinput_xi_change_hierarchy(&[xinput::HierarchyChange {
                len: (2 + name.len().div_ceil(4)) as u16,
                data: xinput::HierarchyChangeData::AddMaster(
                    xinput::HierarchyChangeDataAddMaster {
                        name: name.into_bytes(),
                        send_core: true,
                        enable: true,
                    },
                ),
            }])?
            .check()?;
        let device = connection
            .xinput_xi_query_device(xinput::Device::ALL_MASTER)?
            .reply()?
            .infos
            .into_iter()
            .find(|one| one.name == master_name.as_bytes())
            .ok_or("X11 did not report the new pointer")?
            .deviceid;
        let pointer = Self {
            connection,
            root,
            device,
            cursor,
        };
        // Window zero sets the client pointer of this connection only.
        pointer
            .connection
            .xinput_xi_set_client_pointer(x11rb::NONE, device)?
            .check()?;
        pointer
            .connection
            .xinput_xi_change_cursor(root, cursor, device)?
            .check()?;
        Ok(pointer)
    }

    pub fn position(&self) -> Result<(i32, i32), Error> {
        let reply = self
            .connection
            .xinput_xi_query_pointer(self.root, self.device)?
            .reply()?;
        Ok((reply.root_x >> 16, reply.root_y >> 16))
    }

    pub fn move_to(&self, x: i32, y: i32) -> Result<(), Error> {
        self.connection
            .xtest_fake_input(
                xproto::MOTION_NOTIFY_EVENT,
                0,
                0,
                self.root,
                x.try_into()?,
                y.try_into()?,
                0,
            )?
            .check()?;
        // A window may define its own cursor. Set the agent artwork for this device in
        // each window beneath the pointer, leaving their regular cursor untouched.
        let mut window = self.root;
        for _ in 0..32 {
            let reply = self
                .connection
                .xinput_xi_query_pointer(window, self.device)?
                .reply()?;
            if reply.child == x11rb::NONE {
                break;
            }
            window = reply.child;
            if let Ok(changed) =
                self.connection
                    .xinput_xi_change_cursor(window, self.cursor, self.device)
            {
                let _ = changed.check();
            }
        }
        Ok(())
    }

    pub fn button(&self, button: u8, pressed: bool) -> Result<(), Error> {
        let event = if pressed {
            xproto::BUTTON_PRESS_EVENT
        } else {
            xproto::BUTTON_RELEASE_EVENT
        };
        self.connection
            .xtest_fake_input(event, button, 0, self.root, 0, 0, 0)?
            .check()?;
        Ok(())
    }
}

impl Drop for AgentPointer {
    fn drop(&mut self) {
        if let Ok(removed) =
            self.connection
                .xinput_xi_change_hierarchy(&[xinput::HierarchyChange {
                    len: 3,
                    data: xinput::HierarchyChangeData::RemoveMaster(
                        xinput::HierarchyChangeDataRemoveMaster {
                            deviceid: self.device,
                            return_mode: xinput::ChangeMode::FLOAT,
                            return_pointer: 0,
                            return_keyboard: 0,
                        },
                    ),
                }])
        {
            let _ = removed.check();
        }
    }
}

fn make_cursor(connection: &RustConnection, root: xproto::Window) -> Result<xproto::Cursor, Error> {
    let (width, height) = (52u16, 43u16);
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="52" height="43">
      <rect x="1" y="1" width="47" height="19" rx="6" fill="#ff982e" stroke="#241303"/>
      <path d="M5 19h10l-7 5z" fill="#ff982e"/>
      <text x="24.5" y="14" text-anchor="middle" font-size="11" font-family="sans-serif" font-weight="bold" fill="#241303">Agent</text>
      <path d="M3 25v13l4-4 3 7 3-1-3-7h6z" fill="#ff982e" stroke="#241303" stroke-width="1.4"/>
    </svg>"##;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options)?;
    let mut image = resvg::tiny_skia::Pixmap::new(width.into(), height.into())
        .ok_or("Could not allocate cursor artwork")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut image.as_mut(),
    );

    let format = connection
        .render_query_pict_formats()?
        .reply()?
        .formats
        .into_iter()
        .find(|format| {
            format.type_ == render::PictType::DIRECT
                && format.depth == 32
                && format.direct.alpha_mask == 255
                && format.direct.alpha_shift == 24
                && format.direct.red_shift == 16
                && format.direct.green_shift == 8
                && format.direct.blue_shift == 0
        })
        .ok_or("X11 has no ARGB cursor format")?
        .id;
    let bytes: Vec<_> = image
        .data()
        .chunks_exact(4)
        .flat_map(|pixel| {
            if connection.setup().image_byte_order == xproto::ImageOrder::LSB_FIRST {
                [pixel[2], pixel[1], pixel[0], pixel[3]]
            } else {
                [pixel[3], pixel[0], pixel[1], pixel[2]]
            }
        })
        .collect();
    let pixmap = connection.generate_id()?;
    let gc = connection.generate_id()?;
    let picture = connection.generate_id()?;
    let cursor = connection.generate_id()?;
    connection
        .create_pixmap(32, pixmap, root, width, height)?
        .check()?;
    connection
        .create_gc(gc, pixmap, &xproto::CreateGCAux::new())?
        .check()?;
    connection
        .put_image(
            xproto::ImageFormat::Z_PIXMAP,
            pixmap,
            gc,
            width,
            height,
            0,
            0,
            0,
            32,
            &bytes,
        )?
        .check()?;
    connection
        .render_create_picture(picture, pixmap, format, &render::CreatePictureAux::new())?
        .check()?;
    connection
        .render_create_cursor(cursor, picture, 3, 25)?
        .check()?;
    connection.render_free_picture(picture)?.check()?;
    connection.free_gc(gc)?.check()?;
    connection.free_pixmap(pixmap)?.check()?;
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::Event;

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn moving_and_clicking_leave_the_users_pointer_in_place() {
        let (client, screen) = x11rb::connect(None).unwrap();
        let root = client.setup().roots[screen].root;
        let window = client.generate_id().unwrap();
        client
            .create_window(
                0,
                window,
                root,
                40,
                40,
                100,
                100,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                0,
                &xproto::CreateWindowAux::new().event_mask(xproto::EventMask::BUTTON_PRESS),
            )
            .unwrap()
            .check()
            .unwrap();
        client.map_window(window).unwrap().check().unwrap();
        let user_pointer = || {
            let reply = client.query_pointer(root).unwrap().reply().unwrap();
            (reply.root_x, reply.root_y)
        };
        let original = user_pointer();
        let agent = AgentPointer::start().unwrap();
        agent.move_to(72, 80).unwrap();
        assert_eq!(agent.position().unwrap(), (72, 80));
        assert_eq!(user_pointer(), original);
        agent.button(1, true).unwrap();
        agent.button(1, false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if let Some(Event::ButtonPress(event)) = client.poll_for_event().unwrap() {
                assert_eq!(event.event, window);
                assert_eq!((event.root_x, event.root_y), (72, 80));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the agent click never reached the window"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(user_pointer(), original);
    }
}
