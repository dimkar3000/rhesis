# Changelog

All notable changes to this project will be documented in this file.

## [0.5.1] - 2026-10-28
- fix: background and autostart switches wait for the Apply button like the other settings
- fix: enabling autostart also enables and locks background hosting until autostart is turned off
- fix: installs left with autostart on but background off get paired on the next Apply
- chore: settings list the autostart switch above the background switch
- chore: flatpak builds pinned to the runtime Qt mirror a host JDK into the sandbox for the trimmed JRE

## [0.5.0] - 2026-10-27
- feat: support external language server
- chore: update Readme
- feat: run in the background
- fix: no window flash for hidden-to-tray launches
- fix: AppImage autostart follows the image if it is moved or renamed
- fix: app icon missing in Discover's app listing

## [0.4.0] - 2026-09-05
- fix: server will always be killed before terminating the app
- fix: scrollable settings
- fix: appimage + ci fixes
- feat: formating + clippy + safety comments
- fix: releases file generator in CI
- feat: introduce language switching in settings
- fix: fix translation missmatch in flatpak
- LanguageTool proper managment and status indicator in UI

## [0.3.0] - 2026-08-04

- add more discribution methods. Now we support Flatpak, Appimage, normal installations
- impoved build time for local development
- Updated file discovery path to reflect better on open desktop standards

## [0.2.0] - 2026-07-14
- small fixes
- implemented custom colors for suggestion + started on translations

## [0.1.0] - 2026-07-04

- Grammar, style, and spell checking for over 25 languages
- Automatic language detection, just start typing
- 100 % offline, your writing stays private
- Native KDE desktop experience