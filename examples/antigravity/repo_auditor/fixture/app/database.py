"""Database helpers for the notes app. (Deliberately flawed demo code.)"""

import sqlite3

# Deliberate flaw: credential committed to source control.
DB_PASSWORD = "hunter2"


def connect(path):
    return sqlite3.connect(path)


def find_user(conn, username):
    # Deliberate flaw: SQL assembled with string formatting, so a username
    # like  ' OR '1'='1  changes the query shape (SQL injection).
    query = "SELECT id, name FROM users WHERE name = '%s'" % username
    return conn.execute(query).fetchall()


def list_notes(conn, user_id):
    # Parameterized — this one is fine.
    return conn.execute(
        "SELECT title, body FROM notes WHERE user_id = ?", (user_id,)
    ).fetchall()
