# rules

Principles and design values.

## performance 

Sub-millisecond performance is absolutely non-negotiable.

## degradation

No fidelity can ever compensate for bad timing. Degrade before delay.

## allocation

Never allocate in the audio thread. The hot path is sacred.

## deep API

Lua API must be a rich and intuitive interface for the whole audio engine. We respect the beginner's right to safety, and an expert's right to danger.

## safety

Diligently ensure that all altered system settings are restored upon exit. We may be greedy for resources while running, but should exit as polite guests on termination. Uninstall must be comprehensive.

## privacy

Any telemetry, when extant, must be 100% opt-in and fully transparent.

## architecture

ELM defines `model` is the authoritative state of the application, acted upon solely by `update`, while `view`s are derived exclusively from `model` reads, and may only alter `model` through `message`s sent to `update`.

A new user interface is merely a `view` which can read `state` and send a `message`, allowing a proliferation of designs and use cases.

