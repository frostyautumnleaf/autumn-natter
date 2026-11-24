// A live check of the scan: all of the way the text of a computer takes,
// from the plain files of the older format into the databases.
//
// A user who starts this build on a computer that already holds the directories of
// the older keeping must find their chats, their repositories, their servers and
// their conversations where they left them. This check lays that computer out in a folder
// of its own, starts the engine over it, and reads all of it back out of
// the databases.
//
//     cargo test -p autumn-natter-core --test scan

use autumn_natter_core::config::{self, Config};
use autumn_natter_core::contexts;
use autumn_natter_core::db;
use autumn_natter_core::migrate;
use autumn_natter_core::planning;
use autumn_natter_core::store::{Chat, ChatMeta, Store};
use std::fs;
use std::path::PathBuf;

// A directory of the computer for one check. `--data-dir` is read once for a run, so every
// check of this file shares the folder under it and each one is informed what is there.
fn place(what: &str) -> PathBuf {
    let dir = PathBuf::from("uidata").join(format!("hunt-{}", what));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the place of the check is created");
    dir
}

// A computer of the older format: the conversations in a folder, the records of the
// repositories in another, the files of a server in a third, and a context of the model with
// `.packed` at the end of its name.
#[test]
fn a_machine_of_the_older_shape_reads_back_from_the_databases() {
    let data = place("machine");
    config::set_data_dir(data.clone());
    config::ensure_dirs().expect("the directories of the program are created");
    let places = config::paths();

    // The loose chat, and the chat of a repository. The conversations of a repository is here
    // in the folder they were moved out into, as they do after the first step of the scan; the
    // check makes that step itself below.
    let mut loose = Chat::new("", "model-of-the-machine");
    loose.meta.id = String::from("aaaabbbb01");
    loose.meta.title = String::from("A context of the older format");
    loose.meta.updated = 40;
    fs::create_dir_all(&places.chats).expect("the directory of the chats is created");
    fs::write(
        places.chats.join("aaaabbbb01.json"),
        serde_json::to_string(&loose).expect("the chat is written out"),
    )
    .expect("the chat stands");

    // The repository, with its record outside its directory and its conversation in the
    // contexts directory: the form a run before the databases left behind after it had moved
    // them out.
    let repos = places.repositories.join("ledger_a1b2c3d4e5");
    fs::create_dir_all(&repos).expect("the directory of the repository is created");
    fs::write(repos.join("work.md"), "the work of a chat").expect("a file of the work stands");
    fs::create_dir_all(&places.repo_records).expect("the directory of the records is created");
    fs::write(
        places.repo_records.join("a1b2c3d4e5.json"),
        r#"{"id":"a1b2c3d4e5","name":"Ledger","created":3,"last_used":9,"pinned":true}"#,
    )
    .expect("the record stands");
    fs::create_dir_all(places.repo_chats.join("a1b2c3d4e5")).expect("the contexts directory is created");
    let mut of_repo = Chat::new("a1b2c3d4e5", "model-of-the-machine");
    of_repo.meta.id = String::from("aaaabbbb02");
    of_repo.meta.title = String::from("A context of the repository");
    of_repo.meta.updated = 41;
    fs::write(
        places
            .repo_chats
            .join("a1b2c3d4e5")
            .join("aaaabbbb02.json"),
        serde_json::to_string(&of_repo).expect("the chat is written out"),
    )
    .expect("the chat of the repository stands");

    // A context of the model, of the form an older run kept: the compressed form beside the
    // folder, under the name of the conversation.
    let talk = "aaaabbbb01-0123456789abcdef.ctx";
    let contexts_dir = config::contexts_dir(&Config::default());
    fs::write(
        contexts_dir.join(contexts::short_of(talk)),
        vec![3u8; 4096],
    )
    .expect("the compressed form of a conversation remains");

    // The files of a server of the program itself.
    let server = places.data.join("mcp").join("sv01");
    fs::create_dir_all(&server).expect("the directory of the server is created");
    fs::write(server.join("natter_code.py"), "print(1)").expect("the text of the server remains");

    // A file of the directory of chats that does not read as a chat.
    fs::write(places.chats.join("broken.json"), "not a chat").expect("a broken file stands");

    // The plan of the loose chat, in the form a run before the databases kept it: one file to a
    // chat, in the directory of the plans, under the name of the chat with `.md` on it. A name
    // that is not the name of a chat is not one of ours and stays where it is.
    fs::create_dir_all(planning::folder()).expect("the directory of the plans is created");
    fs::write(
        planning::folder().join("aaaabbbb01.md"),
        "# The plan of the older shape\n\nStep one.\n",
    )
    .expect("the plan stands");
    fs::write(planning::folder().join("notes.md"), "not a plan").expect("a note stands");

    // The scan, as the engine makes it at the start of a run.
    let store = Store::new(config::repositories_dir(&Config::default()));
    let told = migrate::run(&Config::default(), &store);
    assert_eq!(told.chats, 2, "both conversations came across");
    assert_eq!(told.repos, 1, "the record came across");
    assert_eq!(told.contexts, 1, "the conversation came across");
    assert_eq!(told.mcp_files, 1, "the text of the server came across");
    assert_eq!(told.plans, 1, "the plan of the chat came across");
    assert_eq!(told.unreadable, 1, "the file that did not read is informed of");

    // Everything is read back out of the databases.
    let listed = store.list_chats("");
    assert_eq!(listed.len(), 1, "the loose chat stands");
    assert_eq!(listed[0].meta.title, "A context of the older format");
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
    // The plan reads back out of the table of the plans, under the name the chat carries, which
    // is the name of the file it was in before.
    let name = planning::file_name("aaaabbbb01");
    assert_eq!(name, "aaaabbbb01.md");
    let held = planning::words(&name).expect("the plan of the chat is read back");
    assert!(held.contains("# The plan of the older shape"), "{}", held);
    assert!(
        !planning::folder().join("aaaabbbb01.md").exists(),
        "the file of the plan is written as pre-database/ with the rest"
    );
    assert!(
        planning::words(&planning::file_name("aaaabbbb02")).is_none(),
        "a chat that never planned has no row of the plans"
    );
    assert_eq!(
        contexts::held_list(&contexts_dir).len(),
        1,
        "the conversation of the model is in its own database"
    );
    assert!(
        db::open(&places.data.join(db::MCP_DB), db::Kind::Mcp)
            .expect("the database of the servers is opened")
            .read(|conn| Ok(conn
                .prepare("SELECT 1 FROM mcp_files WHERE server_id = 'sv01'")
                .map_err(db::fault)?
                .exists([])
                .map_err(db::fault)?))
            .expect("the text of the server is reads back"),
        "the text of the server is in their own database"
    );

    // The settings, the logs and the files of the work are left as plain files, and the
    // directories of the older format are removed.
    assert!(places.settings_file.starts_with(&data));
    assert!(places.logs.is_dir(), "the logs stay a directory of files");
    assert!(repos.join("work.md").is_file(), "the working directory of a chat stays in the repository");
    assert!(!places.chats.exists(), "the directory of the chats is removed");
    assert!(!places.repo_chats.exists(), "the contexts directory is removed");
    assert!(!places.repo_records.exists(), "the directory of the records is removed");
    assert!(!places.data.join("mcp").exists(), "the directory of the servers is removed");
    assert!(
        data.join(migrate::BEFORE).join("chats").join("broken.json").is_file(),
        "the file that did not read is kept where a person may look at it"
    );

    // The marker remains, and a second start walks nothing.
    assert!(migrate::already_ran(&data), "the search marker remains");
    let again = migrate::run(&Config::default(), &store);
    assert!(again.is_empty(), "the scan is not made twice");
    assert_eq!(store.list_chats("").len(), 1, "nothing came twice");

    // A chat written after the scan goes into the database, and nothing is in the directories
    // of the older format.
    let mut fresh = Chat::new("", "model-of-the-machine");
    fresh.meta = ChatMeta {
        id: String::from("aaaabbbb03"),
        title: String::from("A chat of this build"),
        updated: 60,
        ..Default::default()
    };
    store.save_chat(&fresh).expect("the chat is written");
    assert_eq!(store.list_chats("").len(), 2, "the new chat is with the old");
    assert!(!places.chats.exists(), "no folder of the older format returned");
    drop(places);
    let _ = fs::remove_dir_all(&data);
}
