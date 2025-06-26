import subprocess
from basics import *


class mdk4:
    def __init__(self, interface: str = None, mode: str = None, count: str = None, bssid: str = None):
        self.mode = mode
        self.count = str(count)
        self.bssid = bssid
        self.interface = interface

    def Jammer(self):
        clrscr()
        typewriter.yellow_bright("Jammer Started!\n\n")
        subprocess.run(["sudo", "mdk4", self.interface, self.mode])

    def Beacon_Flooding(self):
        clrscr()
        typewriter.yellow_bright("Beacon Is Flooding!\n\n")
        subprocess.run(["sudo", "mdk4", self.interface, self.mode])

def airmon_scan(interface: str = None):
    clrscr()
    typewriter.yellow_bright("Scanner Started!\n\n")

    if not os.path.exists("Saves"):
        os.makedirs("Saves")

    extensions = ["csv", "kismet.csv", "netxml", "log.csv"]

    for ext in extensions:
        file = f"Saves/networks-{ext}" if ext != "csv" else "Saves/networks-01.csv"
        if os.path.exists(file):
            os.remove(file)

    try:
        subprocess.run([
            "sudo", "airodump-ng", interface,
            "--write", "Saves/networks",
            "--write-interval", "1",
            "--output-format", "csv"
        ])
    except KeyboardInterrupt:
        typewriter.yellow_bright("\nScan Stopped!")
        return
