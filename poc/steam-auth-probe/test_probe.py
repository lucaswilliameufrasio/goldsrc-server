import ctypes
from pathlib import Path
import tempfile
import unittest

import probe


class ProbeTests(unittest.TestCase):
    def test_supported_architectures(self):
        self.assertEqual(probe.library_dir("x86_64"), "linux64")
        self.assertEqual(probe.library_dir("aarch64"), "linuxarm64")
        with self.assertRaises(ValueError):
            probe.library_dir("i386")

    def test_initialization_versions_are_double_nul_terminated(self):
        with tempfile.TemporaryDirectory() as work:
            directory = Path(work)
            definitions = []
            for name in probe.INTERFACES:
                value = "SteamGameServer015" if name == "STEAMGAMESERVER_INTERFACE_VERSION" else name
                definitions.append(f'#define {name} "{value}"')
            header = directory / "interfaces.h"
            header.write_text("\n".join(definitions))
            (directory / "._interfaces.h").write_bytes(b"\x00metadata")
            versions = probe.interface_versions(directory)
            self.assertTrue(versions.endswith(b"\0\0"))
            self.assertEqual(len(versions.split(b"\0")), len(probe.INTERFACES) + 2)
            header.write_text(header.read_text().replace("SteamGameServer015", "SteamGameServer999"))
            with self.assertRaises(ValueError):
                probe.interface_versions(directory)

    def test_missing_interfaces_fail_closed(self):
        with tempfile.TemporaryDirectory() as work:
            with self.assertRaises(KeyError):
                probe.interface_versions(Path(work))

    def test_native_callback_layout_matches_linux_sdk_pack_four(self):
        self.assertEqual(ctypes.sizeof(ctypes.c_void_p), 8)
        self.assertEqual(ctypes.sizeof(probe.Callback), 20)
        self.assertEqual(probe.Callback.data.offset, 8)
        self.assertEqual(probe.Callback.size.offset, 16)

    def test_wrong_architecture_or_non_elf_fails_before_loading(self):
        with tempfile.TemporaryDirectory() as work:
            library = Path(work) / "library.so"
            header = bytearray(20)
            header[:6] = b"\x7fELF\x02\x01"
            header[18:20] = (62).to_bytes(2, "little")
            library.write_bytes(header)
            probe.validate_library(library, "x86_64")
            with self.assertRaises(ValueError):
                probe.validate_library(library, "aarch64")
            library.write_bytes(b"not an ELF")
            with self.assertRaises(ValueError):
                probe.validate_library(library, "x86_64")


if __name__ == "__main__":
    unittest.main()
