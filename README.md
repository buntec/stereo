# Stereo 📻

Curate and play music in your browser.
No account, login or subscription required!

Works across platforms in any reasonably modern browser.
Tested with Firefox and Chrome.

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

<img width="1840" height="1195" alt="Screenshot 2026-01-03 at 18 04 40" src="https://github.com/user-attachments/assets/feb98b68-ef28-4d99-9c86-91da630edddd" />


# Development

Don't forget to rebuild the frontend (`just build-frontend`) before committing frontend changes!
