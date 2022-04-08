import numpy as np
from matplotlib import pyplot as plt

# args to create data from copy past logs:
# cat hb_timeout.txt | uniq -u | grep "timeout in" | cut -d " " -f 1,6 |
# cut -d ":" -f 3- >> hb_timeout.dat

data = np.loadtxt("hb_timeout.dat")
# print(

fig = plt.figure(figsize=(9, 4))
plt.scatter(data[:, 0], data[:, 1])
plt.plot(data[:, 0], data[:, 1], alpha=0.5)
plt.axhspan(0, 50, alpha=0.4, color="red")

idx = np.argmin(data[:, 1])
plt.annotate(f"lowest: {data[idx,1]}ms", xy=data[idx], xytext=(40, 100),
             arrowprops=dict(arrowstyle='-', facecolor="black"),)

plt.xlabel("time in seconds")
plt.ylabel("milliseconds till heartbeat timeout")
plt.savefig("hb_timeout.png", dpi=300)
