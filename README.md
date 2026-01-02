# Stereo 📻

Curate and play music in your browser. No account, login or subscription required!

# Getting started

All you need is [uv](https://github.com/astral-sh/uv).

Launch Stereo from the command-line:

```
uvx git+https://github.com/buntec/stereo
```

Navigate to `localhost:8005` in your browser.

Begin by searching for and adding music to a new collection.
Or by importing from another collection.
You can create as many collections as you want.
A collection is just a file on your local hard disk.
More precisely, it's a SQLite database containing track meta data only - no actual audio data is stored or downloaded!

For the best possible playback experience, a YouTube Premium subscription or a good ad-blocker is highly recommended.

Work in progress! 🚧

<img width="1840" height="1195" alt="Screenshot 2025-12-31 at 16 57 06" src="https://github.com/user-attachments/assets/5c08b099-6b7f-4517-b11c-6c1dcd44f199" />

# Development

Don't forget to rebuild the frontend (`just build-frontend`) before committing frontend changes!
