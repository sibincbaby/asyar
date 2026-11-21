### 8.41 `MediaService` — Now playing and transport control

**Runs in:** both worker and view.

**Permission required:** `media:control` for all three methods.

Read what is playing in a local media player and send play, pause, and skip commands. Works on Linux via MPRIS over D-Bus. A player that is not running is never launched.

| Platform | Players                                          | `playerId` accepts                                                                                 |
| -------- | ------------------------------------------------ | -------------------------------------------------------------------------------------------------- |
| Linux    | Any MPRIS player (Spotify, VLC, mpv, Firefox, …) | The full bus name (`org.mpris.MediaPlayer2.spotify`) or its suffix (`"spotify"`), case-insensitive |

`MediaSessionInfo.playerId` is the player's MPRIS bus name. Round-trip it into `control()` instead of building ids by hand.

```typescript
type PlaybackStatus = 'playing' | 'paused' | 'stopped';
type MediaTransportAction = 'playPause' | 'play' | 'pause' | 'next' | 'previous';

interface MediaSessionInfo {
  playerId: string; // MPRIS bus name, e.g. "org.mpris.MediaPlayer2.spotify"
  playerName: string; // e.g. "Spotify"
  status: PlaybackStatus;
  title: string | null;
  artist: string | null;
  album: string | null;
  durationSeconds: number | null;
  positionSeconds: number | null;
}

interface IMediaService {
  getNowPlaying(playerId?: string): Promise<MediaSessionInfo | null>;
  listSessions(): Promise<MediaSessionInfo[]>;
  control(action: MediaTransportAction, playerId?: string): Promise<void>;
}
```

**Manifest declaration:**

```json
{ "permissions": ["media:control"] }
```

**Usage — show the current track:**

```typescript
const media = context.getService<IMediaService>('media');

const track = await media.getNowPlaying(); // best running player
if (track) {
  console.log(`${track.title} — ${track.artist} [${track.playerName}, ${track.status}]`);
}
```

**Usage — target a specific player:**

```typescript
await media.control('next', 'spotify');
const sessions = await media.listSessions(); // only running players with a track
```

**Semantics:**

| Call                              | Behavior                                                                                                        |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `getNowPlaying()` (no `playerId`) | Picks among running players, preferring a `playing` session over `paused`. Resolves `null` if none has a track. |
| `getNowPlaying(id)`               | Queries that player only. `null` if it is not running. Rejects for an unknown player id.                        |
| `listSessions()`                  | Running supported players that are playing or paused. A player that fails to answer is skipped, not fatal.      |
| `control(action)` (no `playerId`) | The active MPRIS player (playing preferred over paused).                                                        |
| `control(action, id)`             | Scripts that player directly. Rejects if the player is not running or the id is unknown.                        |

**How it works under the hood:**

| Platform | Path       | API                                                                                                                                   |
| -------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Linux    | Everything | MPRIS on the session bus: `ListNames` to discover `org.mpris.MediaPlayer2.*`, `Properties.GetAll` to read, Player methods to control. |

With no session bus (for example a bare SSH session) every call rejects with an `MPRIS session bus unavailable` error. Players that run several instances appear as `org.mpris.MediaPlayer2.vlc.instance1234`; the bare name `"vlc"` still matches.

> **Pro Tip:** Prefer `getNowPlaying()` without an id for "what's playing" UIs and pass the returned `playerId` back to `control()` so your buttons act on the same player you are displaying.

---
