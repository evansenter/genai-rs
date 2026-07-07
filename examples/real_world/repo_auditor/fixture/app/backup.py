"""Backup helper for the notes app. (Deliberately flawed demo code.)"""

import os


def backup_notes(archive_name):
    # Deliberate flaw: user-controlled archive_name is interpolated into a
    # shell command, so  "x; rm -rf ~"  executes arbitrary commands.
    os.system("tar czf /tmp/" + archive_name + ".tgz notes/")


def restore_notes(archive_path):
    # Same pattern on the restore path.
    os.system("tar xzf " + archive_path + " -C notes/")
