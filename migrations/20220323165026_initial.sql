CREATE TABLE users(
    ident TEXT PRIMARY KEY,
    token TEXT NOT NULL
);


CREATE TABLE crates(
    name TEXT PRIMARY KEY
);

CREATE TABLE crate_owners(
    id BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    crate_name TEXT NOT NULL,
    user_ident TEXT NOT NULL,

    CONSTRAINT fk_user_ident
		FOREIGN KEY(user_ident)
			REFERENCES users(ident)
				ON UPDATE CASCADE
					ON DELETE CASCADE,

    CONSTRAINT fk_crate_name
		FOREIGN KEY(crate_name)
			REFERENCES crates(name)
				ON UPDATE CASCADE
					ON DELETE CASCADE
);
