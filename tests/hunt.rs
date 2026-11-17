// A live check of the hunt: the whole of the way the words of a machine take,
// from the plain files of the older shape into the databases.
//
// A user who starts this build on a machine that already holds the folders of
// the older keeping must find their chats, their repositories, their servers and
// their talks where they left them. This check lays that machine out in a folder
// of its own, starts the engine over it, and reads the whole of it back out of
// the databases.
//
//     cargo test -p autumn-natter-core --test hunt

use autumn_natter_core::config::{self, Config};
use autumn_natter_core::contexts;
use autumn_natter_core::db;
use autumn_natter_core::migrate;
use autumn_natter_core::store::{Chat, ChatMeta, Store};
use std::fs;
use std::path::PathBuf;

// A folder of the machine for one check. `--data-dir` is read once for a run, so
// every check of this file shares the folder under it and each one is told what
// stands there.
fn place(what: &str) -> PathBuf {
    let dir = PathBuf::from("uidata").join(format!("hunt-{}", what));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the place of the check is made");
    dir
}

// A machine of the older shape: the talks in a folder, the records of the
// repositories in another, the files of a server in a third, and a talk of the
// model with `.packed` at the end of its name.
#[test]
fn a_machine_of_the_older_shape_reads_back_from_the_databases() {
    let data = place("machine");
    config::set_data_dir(data.clone());
    config::ensure_dirs().expect("the folders of the program are made");
    let places = config::paths();

    // The loose chat, and the chat of a repository. The talks of a repository
    // stand here in the folder they were moved out into, as they do after the
    // first step of the hunt; the check makes that step itself below.
    let mut loose = Chat::new("", "model-of-the-machine");
    loose.meta.id = String::from("aaaabbbb01");
    loose.meta.title = String::from("A talk of the older shape");
    loose.meta.updated = 40;
    fs::create_dir_all(&places.chats).expect("the folder of the chats is made");
    fs::write(
        places.chats.join("aaaabbbb01.json"),
        serde_json::to_string(&loose).expect("the chat is written out"),
    )
    .expect("the chat stands");

    // The repository, with its record outside its directory and its talk in the
    // folder of the talks: the shape a run before the databases left behind after
    // it had moved them out.
    let repos = places.repositories.join("ledger_a1b2c3d4e5");
    fs::create_dir_all(&repos).expect("the directory of the repository is made");
    fs::write(repos.join("work.md"), "the work of a chat").expect("the work of the chat stands");
    fs::create_dir_all(&places.repo_records).expect("the folder of the records is made");
    fs::write(
        places.repo_records.join("a1b2c3d4e5.json"),
        r#"{"id":"a1b2c3d4e5","name":"Ledger","created":3,"last_used":9,"pinned":true}"#,
    )
    .expect("the record stands");
    fs::create_dir_all(places.repo_chats.join("a1b2c3d4e5")).expect("the folder of the talks is made");
    let mut of_repo = Chat::new("a1b2c3d4e5", "model-of-the-machine");
    of_repo.meta.id = String::from("aaaabbbb02");
    of_repo.meta.title = String::from("A talk of the repository");
    of_repo.meta.updated = 41;
    fs::write(
        places
            .repo_chats
            .join("a1b2c3d4e5")
            .join("aaaabbbb02.json"),
        serde_json::to_string(&of_repo).expect("the chat is written out"),
    )
    .expect("the chat of the repository stands");

    // A talk of the model, of the shape an older run kept: the short shape beside
    // the folder, under the name of the talk.
    let talk = "aaaabbbb01-0123456789abcdef.ctx";
    let contexts_dir = config::contexts_dir(&Config::default());
    fs::write(
        contexts_dir.join(contexts::short_of(talk)),
        vec![3u8; 4096],
    )
    .expect("the short shape of a talk stands");

    // The files of a server of the program itself.
    let server = places.data.join("mcp").join("sv01");
    fs::create_dir_all(&server).expect("the folder of the server is made");
    fs::write(server.join("natter_code.py"), "print(1)").expect("the words of the server stand");

    // A file of the folder of chats that does not read as a chat.
    fs::write(places.chats.join("broken.json"), "not a chat").expect("a broken file stands");

    // The hunt, as the engine makes it at the start of a run.
    let store = Store::new(config::repositories_dir(&Config::default()));
    let told = migrate::run(&Config::default(), &store);
    assert_eq!(told.chats, 2, "both talks came across");
    assert_eq!(told.repos, 1, "the record came across");
    assert_eq!(told.contexts, 1, "the talk came across");
    assert_eq!(told.mcp_files, 1, "the words of the server came across");
    assert_eq!(told.unreadable, 1, "the file that did not read is told of");

    // Everything is read back out of the databases.
    let listed = store.list_chats("");
    assert_eq!(listed.len(), 1, "the loose chat stands");
    assert_eq!(listed[0].meta.title, "A talk of the older shape");
    let of_repo = store.list_chats("a1b2c3d4e5");
    assert_eq!(of_repo.len(), 1, "the chat of the repository stands");
    assert_eq!(of_repo[0].meta.id, "aaaabbbb02");
    assert_eq!(
        store
            .load_chat("aaaabbbb02")
            .expect("the chat of the repository is read by its id alone")
            .meta
            .repo_id,
        "a1b2c3d4e5"
    );
    let repos_list = store.scan_repos();
    assert_eq!(repos_list.len(), 1, "the repository stands");
    assert_eq!(repos_list[0].name, "Ledger");
    assert!(repos_list[0].pinned, "the pin came across");
    assert_eq!(
        contexts::held_list(&contexts_dir).len(),
        1,
        "the talk of the model stands in its own database"
    );
    assert!(
        db::open(&places.data.join(db::MCP_DB), db::Kind::Mcp)
            .expect("the database of the servers is opened")
            .read(|conn| Ok(conn
                .prepare("SELECT 1 FROM mcp_files WHERE server_id = 'sv01'")
                .map_err(db::fault)?
                .exists([])
                .map_err(db::fault)?))
            .expect("the words of the server are read back"),
        "the words of the server stand in their own database"
    );

    // The settings, the logs and the work of a chat are left as plain files, and
    // the folders of the older shape are gone.
    assert!(places.settings_file.starts_with(&data));
    assert!(places.logs.is_dir(), "the logs stay a folder of files");
    assert!(repos.join("work.md").is_file(), "the work of a chat stays in the repository");
    assert!(!places.chats.exists(), "the folder of the chats is gone");
    assert!(!places.repo_chats.exists(), "the folder of the talks is gone");
    assert!(!places.repo_records.exists(), "the folder of the records is gone");
    assert!(!places.data.join("mcp").exists(), "the folder of the servers is gone");
    assert!(
        data.join(migrate::BEFORE).join("chats").join("broken.json").is_file(),
        "the file that did not read is kept where a person may look at it"
    );

    // The mark stands, and a second start walks nothing.
    assert!(migrate::already_ran(&data), "the mark of the hunt stands");
    let again = migrate::run(&Config::default(), &store);
    assert!(again.is_empty(), "the hunt is not made twice");
    assert_eq!(store.list_chats("").len(), 1, "nothing came twice");

    // A chat written after the hunt goes into the database, and nothing stands in
    // the folders of the older shape.
    let mut fresh = Chat::new("", "model-of-the-machine");
    fresh.meta = ChatMeta {
        id: String::from("aaaabbbb03"),
        title: String::from("A chat of this build"),
        updated: 60,
        ..Default::default()
    };
    store.save_chat(&fresh).expect("the chat is written");
    assert_eq!(store.list_chats("").len(), 2, "the new chat stands with the old");
    assert!(!places.chats.exists(), "no folder of the older shape came back");
    drop(places);
    let _ = fs::remove_dir_all(&data);
}
