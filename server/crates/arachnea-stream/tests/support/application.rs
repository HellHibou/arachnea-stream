pub(crate) fn configure_test_application_data_dir() {
    let _ =
        arachnea_core::application::configure_application_data_dir_name("arachnea-stream-tests");
}
