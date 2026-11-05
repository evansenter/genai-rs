use crate::client::Client;

#[test]
fn store_list_builder_setters_replace() {
    let client = Client::new("k".to_string());
    let list = client.file_search_stores().list();
    assert_eq!(list.page_size, None);
    assert_eq!(list.page_token, None);

    let list = list
        .with_page_size(20)
        .with_page_token("t1")
        .with_page_size(1)
        .with_page_token(String::from("t2"));
    assert_eq!(list.page_size, Some(1));
    assert_eq!(list.page_token.as_deref(), Some("t2"));
}

#[test]
fn document_list_builder_owns_its_store_name_and_query() {
    let client = Client::new("k".to_string());
    let store_name = String::from("fileSearchStores/abc");
    let list = client.file_search_stores().documents().list(&store_name);
    // The builder owns the store name, so the caller's string can go.
    drop(store_name);
    assert_eq!(list.store_name, "fileSearchStores/abc");
    assert_eq!(list.page_size, None);
    assert_eq!(list.page_token, None);

    let list = list
        .with_page_size(20)
        .with_page_token("t1")
        // `with_*` replaces.
        .with_page_size(2)
        .with_page_token("t2");
    assert_eq!(list.page_size, Some(2));
    assert_eq!(list.page_token.as_deref(), Some("t2"));
}
