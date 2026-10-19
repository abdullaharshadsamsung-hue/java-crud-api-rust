use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use migrated_service::error::AppError;
use migrated_service::models::user::{
    User, UserValidationError, CREATE_USER_TABLE_SQL, NAME_NOT_BLANK_MESSAGE, USER_SELECT_COLUMNS,
};

fn hash_of(u: &User) -> u64 {
    let mut h = DefaultHasher::new();
    u.hash(&mut h);
    h.finish()
}

fn full_user() -> User {
    User::new(
        7,
        Some("Alice".to_string()),
        Some("alice@x.com".to_string()),
        Some("secret".to_string()),
        Some("ROLE_USER".to_string()),
        Some("about me".to_string()),
    )
}

#[tokio::test]
async fn no_args_constructor_all_defaults() {
    let u = User::default();
    assert_eq!(u.id(), 0);
    assert!(u.name().is_none());
    assert!(u.email().is_none());
    assert!(u.password().is_none());
    assert!(u.role().is_none());
    assert!(u.about().is_none());
}

#[tokio::test]
async fn all_args_constructor_sets_fields_in_order() {
    let u = full_user();
    assert_eq!(u.id(), 7);
    assert_eq!(u.name(), Some("Alice"));
    assert_eq!(u.email(), Some("alice@x.com"));
    assert_eq!(u.password(), Some("secret"));
    assert_eq!(u.role(), Some("ROLE_USER"));
    assert_eq!(u.about(), Some("about me"));
}

#[tokio::test]
async fn all_args_constructor_does_not_validate() {
    let u = User::new(1, None, None, None, None, None);
    assert_eq!(u.id(), 1);
    assert!(u.validate().is_err());
    let u2 = User::new(2, Some("   ".to_string()), None, None, None, None);
    assert_eq!(u2.name(), Some("   "));
    assert!(u2.validate().is_err());
}

#[tokio::test]
async fn builder_sets_given_fields_only() {
    let u = User::builder().name("A").email("a@x.com").build();
    assert_eq!(u.id(), 0);
    assert_eq!(u.name(), Some("A"));
    assert_eq!(u.email(), Some("a@x.com"));
    assert!(u.password().is_none());
    assert!(u.role().is_none());
    assert!(u.about().is_none());
}

#[tokio::test]
async fn builder_all_fields_and_new_instances() {
    let b = User::builder()
        .id(7)
        .name("Alice")
        .email("alice@x.com")
        .password("secret")
        .role("ROLE_USER")
        .about("about me");
    let u1 = b.clone().build();
    let mut u2 = b.build();
    assert_eq!(u1, full_user());
    u2.set_name(Some("Changed".to_string()));
    assert_eq!(u1.name(), Some("Alice"));
}

#[tokio::test]
async fn setters_and_getters_roundtrip() {
    let mut u = User::default();
    u.set_id(42);
    assert_eq!(u.id(), 42);
    u.set_name(Some("n".to_string()));
    assert_eq!(u.name(), Some("n"));
    u.set_email(Some("e@x".to_string()));
    assert_eq!(u.email(), Some("e@x"));
    u.set_password(Some("p".to_string()));
    assert_eq!(u.password(), Some("p"));
    u.set_role(Some("r".to_string()));
    assert_eq!(u.role(), Some("r"));
    u.set_about(Some("a".to_string()));
    assert_eq!(u.about(), Some("a"));
}

#[tokio::test]
async fn setters_accept_null_and_mutate_only_one_field() {
    let base = full_user();
    let mut u = base.clone();
    u.set_name(None);
    assert!(u.name().is_none());
    assert_eq!(u.email(), base.email());
    assert_eq!(u.id(), base.id());
    assert_eq!(u.about(), base.about());

    let mut u = base.clone();
    u.set_about(None);
    assert!(u.about().is_none());
    assert_eq!(u.name(), base.name());

    // no transformation
    let mut u = base.clone();
    u.set_email(Some("  MiXeD@X.com ".to_string()));
    assert_eq!(u.email(), Some("  MiXeD@X.com "));
}

#[tokio::test]
async fn equality_and_hash() {
    let a = full_user();
    let b = full_user();
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));

    let variants: Vec<User> = vec![
        { let mut u = a.clone(); u.set_id(8); u },
        { let mut u = a.clone(); u.set_name(Some("B".into())); u },
        { let mut u = a.clone(); u.set_email(None); u },
        { let mut u = a.clone(); u.set_password(Some("x".into())); u },
        { let mut u = a.clone(); u.set_role(Some("ADMIN".into())); u },
        { let mut u = a.clone(); u.set_about(Some("other".into())); u },
    ];
    for v in variants {
        assert_ne!(a, v);
    }
    assert_ne!(a, User::default());
}

#[tokio::test]
async fn to_string_format() {
    assert_eq!(
        full_user().to_string(),
        "User(id=7, name=Alice, email=alice@x.com, password=secret, role=ROLE_USER, about=about me)"
    );
    assert_eq!(
        User::default().to_string(),
        "User(id=0, name=null, email=null, password=null, role=null, about=null)"
    );
    assert!(full_user().to_string().contains("password=secret"));
}

#[tokio::test]
async fn validation_accepts_non_blank_name() {
    assert!(User::builder().name("Bob").build().validate().is_ok());
    assert!(User::builder().name(" x ").build().validate().is_ok());
    // other fields unconstrained
    assert!(User::builder().name("Bob").about("a".repeat(1000)).build().validate().is_ok());
}

#[tokio::test]
async fn validation_rejects_blank_names() {
    let cases = vec![
        User::default(),
        User::builder().name("").build(),
        User::builder().name("   ").build(),
        User::builder().name("\t\n").build(),
    ];
    for u in cases {
        let err = u.validate().unwrap_err();
        assert_eq!(
            err,
            UserValidationError { field: "name", message: NAME_NOT_BLANK_MESSAGE }
        );
        assert_eq!(err.message, "please Add the department Name");
        assert_eq!(err.to_string(), "name: please Add the department Name");
    }
}

#[tokio::test]
async fn validation_error_maps_to_bad_request() {
    let err = User::default().validate().unwrap_err();
    let app_err: AppError = err.into();
    match app_err {
        AppError::BadRequest(msg) => assert!(msg.contains(NAME_NOT_BLANK_MESSAGE)),
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn json_roundtrip() {
    let u = full_user();
    let v = serde_json::to_value(&u).unwrap();
    assert_eq!(v["id"], 7);
    assert_eq!(v["name"], "Alice");
    assert_eq!(v["password"], "secret");
    let back: User = serde_json::from_value(v).unwrap();
    assert_eq!(back, u);

    let empty: User = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, User::default());
}

#[tokio::test]
async fn schema_definition() {
    let sql = CREATE_USER_TABLE_SQL;
    assert!(sql.contains("\"user\""));
    assert!(sql.contains("user_id"));
    assert!(sql.contains("SERIAL PRIMARY KEY"));
    assert!(sql.contains("user_email    VARCHAR(255) UNIQUE"));
    assert!(sql.contains("user_about    VARCHAR(500)"));
    for c in ["user_name", "user_password", "user_role"] {
        assert!(sql.contains(c));
    }
    for alias in ["AS id", "AS name", "AS email", "AS password", "AS role", "AS about"] {
        assert!(USER_SELECT_COLUMNS.contains(alias));
    }
}