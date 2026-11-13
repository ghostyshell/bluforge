# Logs

Every backend step, child-process line and UI failure is recorded in a bounded
in-memory buffer and shown on the **Logs** page, newest first. It exists so a
failed rip can be inspected instead of guessed at.

## What lands there

- Core steps: disc open, the ffmpeg argv, the output path, queue transitions.
- Everything a child process writes to stderr (ffmpeg, the Atmos tools, tsMuxeR),
  line by line, with the process as the source and error lines flagged.
- Every failure the interface catches, with its full message.

`RUST_LOG=debug` widens the tracing filter; the page shows info and above by
default.

## Using it

- **Filter** matches message, source, level and detail text. `#123` pins one
  entry, which is what the error banner's **View log** button does.
- **Level** narrows to errors or warnings.
- A row's **detail** disclosure holds multi-line output, such as the verify
  report or a stack of ffmpeg lines.
- **Clear** empties the buffer (2,000 entries is the cap; the oldest go first).

The session panel keeps the last four entries with a link into the same page, so
the result of the thing you just ran is one click away.

## What it is not

- It is not persisted: the buffer dies with the app. Copy the detail before
  closing if you are filing a report.
- It is not the terminal. A debug build still prints to stderr for a live view.
- Keys and passwords are not redacted by the buffer; the backend does not log
  them, but a child tool might echo a path or a URL.
