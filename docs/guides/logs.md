# Logs

Every backend step, child-process line and UI failure is recorded in a bounded
in-memory buffer and shown on the **Logs** page, newest first. It exists so a
failed rip can be inspected instead of guessed at.

## What lands there

- Core steps: disc open, the ffmpeg argv, the output path, queue transitions.
- Everything a child process writes to stderr (ffmpeg, the Atmos tools, tsMuxeR),
  line by line, with the process as the source and error lines flagged.
- Every failure the interface catches, with its full message.
- The reason a step failed, logged where it failed rather than at the boundary:
  an unopenable disc, a refused title select, a failed spawn, a non-zero exit.
- Every failed job (rip, DVD rip, ISO export, clone, author, burn) with its
  context — source, title, mode, destination — followed by the last 40 lines
  the producing process wrote. The banner links straight to that entry.

`RUST_LOG=debug` widens the tracing filter; the page shows info and above by
default.

## Using it

- **Filter** matches message, source, level and detail text. `#123` pins one
  entry, which is what the error banner's **View log** button does.
- **Job** chips narrow to one run. A job opens a section with a start marker,
  tags every line it and its child processes write, and closes with a finished
  marker, so a run can be read on its own instead of interleaved with polling.
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
- Entries are scrubbed before they are stored: the home directory becomes `~`,
  hex blobs of 32 characters or more (keys, hashes) keep their first 8, and a
  URL query string becomes `?…`. A password inside a query string still shows as
  its key name only, never its value.
