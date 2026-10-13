# Device compatibility

| Device family | Current evidence and support |
| --- | --- |
| C64 Ultimate | CLI hardware acceptance on firmware 1.1.0, FPGA 122, core 1.49; see the [recorded tests](hardware-testing.md). GUI/native input acceptance is recorded separately. |
| Ultimate 64 / Elite / Elite II | Potential UCI Network Target compatibility. Software and physical acceptance are pending; the current REST client accepts only the `C64 Ultimate` product identifier. |
| Ultimate-II+ / II+L | Potential UCI Network Target compatibility, with suitable networking and firmware. Software and physical acceptance remain pending. |
| Original Ultimate-II | Unverified; networking hardware and firmware capabilities vary. |

The C64 client requires the documented [UCI Network Target](https://1541u-documentation.readthedocs.io/en/latest/uci/network_target.html). Automatic deployment also requires compatible [REST runner endpoints](https://1541u-documentation.readthedocs.io/en/latest/api/api_calls.html). A matching protocol description is insufficient to claim this application's support. Firmware numbering differs between product families; the Ultimate 3.x REST requirements must not be applied indiscriminately to C64 Ultimate's 1.x branch. No firmware update is performed automatically.
