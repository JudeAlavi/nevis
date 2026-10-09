# Diagrams

A decision with a loop back:

```mermaid
flowchart TD
    A[Start] --> B{Is it working?}
    B -- Yes --> C[Ship it]
    B -- No --> D[Debug]
    D --> E([Read the logs])
    E --> B
    C --> F((Done))
```

Left to right, with Persian labels and every kind of line:

```mermaid
graph LR
    a[یادداشت] -->|ذخیره| b[(پایگاه داده)]
    b -.-> c{{Cache}}
    b ==> d[/Export/]
    a --- e>Flag]
    c --o f[[Subroutine]]
    d <--> f
```

Shapes:

```mermaid
flowchart TD
    s1[Box] --> s2(Round) --> s3([Stadium])
    s1 --> s4[[Subroutine]] --> s5[(Cylinder)]
    s1 --> s6((Circle)) --> s7{Diamond}
    s4 --> s8{{Hexagon}}
    s3 & s5 & s7 & s8 --> s9[A longer label that should wrap onto two lines in the box]
```

A sequence diagram: participants and an actor, every kind of arrow, notes, frames, numbers:

```mermaid
sequenceDiagram
    autonumber
    actor U as User
    participant A as App
    participant S as Server
    U->>A: Open a note
    A->>S: GET /note
    S-->>A: 200 OK
    Note right of A: cached for an hour
    loop Every 5 minutes
        A-)S: sync
    end
    alt saved
        A-->>U: Done
    else failed
        A-xU: Error
    end
    A->>A: tidy up
    Note over U,S: the end of the day
```

A state diagram: a start, an end, a note, and a state with states inside:

```mermaid
stateDiagram-v2
    [*] --> Still
    Still --> [*]
    Still --> Moving : push
    Moving --> Still : stop
    Moving --> Crash
    Crash --> [*]
    note right of Moving : on wheels
    state Crash {
        [*] --> Dented
        Dented --> Towed
        Towed --> [*]
    }
```

None of those, so it stays code:

```mermaid
pie title Pets
    "Dogs" : 386
    "Cats" : 85
```
