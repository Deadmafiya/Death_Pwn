# Death Pwn - Wi-Fi Penetration Testing Toolkit


Death Pwn is a powerful command-line tool for Wi-Fi penetration testing that simplifies common attacks like beacon flooding and deauthentication. Built with Python and leveraging system tools like mdk4, it provides an intuitive interface for security professionals and researchers.

## Key Features
Beacon Flooding Attack: Create fake access points to disrupt network scanning

Deauthentication Attack: Force devices to disconnect from target access points

User-Friendly Interface: Colorful terminal interface with real-time feedback

Monitor Mode Management: Automatic switching between monitor/managed modes

Command Recognition: Smart command parsing with error correction

# Installation
### Prerequisites
- Linux OS (Kali Linux recommended)
- Wireless network adapter supporting monitor mode
- Python 3.7+

### Installation
`https://github.com/Deadmafiya/Death_Pwn/blob/main/install.sh`

### Make it executable:
`chmod +x install.sh`

### Run the installer:
`sudo ./install.sh`

## Run the tool
`sudo python3 Death_Pwn.py`

## Starting the Tool
`sudo python3 Death_Pwn.py`


# Command Syntax
`Death_Pwn#~ [command] [options]`

## Available Commands
- Command	Syntax	Description
### Beacon Flooding:	
- beacon flooding [packet_count]  -Floods the area with fake access points(AP)
- Example use: `start beacon flooding by 100 packets`   -it will start making fake AP with 100ps


### Deauthentication:
- deauth [MAC_ADDRESS]	          -Deauthenticates devices from target network
- Example use:
Command Examples: `deauth this wifi - 00:11:22:33:44:55`  -it will start deauthing users from wifi make users not to use that wifi

### other functions are in the developtment stage



# Troubleshooting
## Common Issues & Solutions
### "wlan1 not found" error:

Check your wireless interface name with `iwconfig`

Update variable named `int_face` in `basics.py` to match your interface like `wlan0` or other if have

### Monitor mode fails to enable:

`sudo airmon-ng check kill`
`sudo airmon-ng start wlan0`  -Replace with your interface


Attacks not working:

Ensure your wireless card supports packet injection

Try closer proximity to the target

Verify you have proper permissions (running with sudo)

Dependencies missing:

Re-run the installation script

Manually install missing components:

bash
sudo apt install -y mdk4 aircrack-ng
pip3 install rich
